using System.Globalization;
using Avalonia.Data.Converters;
using Avalonia.Media;
using LunaticPrincess.Core.Library;

namespace LunaticPrincess.Desktop.Converters;

/// <summary>本地匹配状态 → 徽标颜色。参数 "Background" 取浅色底，"Foreground" 取文字/圆点色。</summary>
public sealed class LocalAvailabilityToBrushConverter : IValueConverter
{
    public string Mode { get; set; } = "Foreground";

    private static readonly IBrush PresentFg = new SolidColorBrush(Color.Parse("#5BDB95"));
    private static readonly IBrush PresentBg = new SolidColorBrush(Color.Parse("#1E3428"));
    private static readonly IBrush NcmFg = new SolidColorBrush(Color.Parse("#F0B64D"));
    private static readonly IBrush NcmBg = new SolidColorBrush(Color.Parse("#332917"));
    private static readonly IBrush MissingFg = new SolidColorBrush(Color.Parse("#FF7D7D"));
    private static readonly IBrush MissingBg = new SolidColorBrush(Color.Parse("#3B1C20"));

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
}

/// <summary>日志级别 → 文本颜色。</summary>
public sealed class LogLevelToBrushConverter : IValueConverter
{
    private static readonly IBrush Info = new SolidColorBrush(Color.Parse("#C2CBD7"));
    private static readonly IBrush Success = new SolidColorBrush(Color.Parse("#5BDB95"));
    private static readonly IBrush Warning = new SolidColorBrush(Color.Parse("#F0B64D"));
    private static readonly IBrush Error = new SolidColorBrush(Color.Parse("#FF7D7D"));

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
}

/// <summary>扫码状态类型 → 状态文本颜色。</summary>
public sealed class StatusKindToBrushConverter : IValueConverter
{
    private static readonly IBrush Waiting = new SolidColorBrush(Color.Parse("#C2CBD7"));
    private static readonly IBrush Scanned = new SolidColorBrush(Color.Parse("#F0B64D"));
    private static readonly IBrush Success = new SolidColorBrush(Color.Parse("#5BDB95"));
    private static readonly IBrush Error = new SolidColorBrush(Color.Parse("#FF7D7D"));

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
