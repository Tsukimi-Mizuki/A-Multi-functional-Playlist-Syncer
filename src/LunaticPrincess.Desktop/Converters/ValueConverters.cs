using System.Globalization;
using Avalonia.Data.Converters;
using Avalonia.Media;
using LunaticPrincess.Core.Library;
using LunaticPrincess.Desktop.Services;

namespace LunaticPrincess.Desktop.Converters;

/// <summary>
/// 本地匹配状态 → 徽标颜色。参数 "Background" 取浅色底，"Foreground" 取文字/圆点色。
/// 画刷实例来自 <see cref="ThemeBrushes"/>，切换主题时其 Color 会被就地更新。
/// </summary>
public sealed class LocalAvailabilityToBrushConverter : IValueConverter
{
    public string Mode { get; set; } = "Foreground";

    private IBrush PresentFg => ThemeBrushes.Success;
    private IBrush PresentBg => ThemeBrushes.SuccessSoft;
    private IBrush NcmFg => ThemeBrushes.Warning;
    private IBrush NcmBg => ThemeBrushes.WarningSoft;
    private IBrush MissingFg => ThemeBrushes.Danger;
    private IBrush MissingBg => ThemeBrushes.DangerSoft;

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
    private static IBrush Info => ThemeBrushes.TextSecondary;
    private static IBrush Success => ThemeBrushes.Success;
    private static IBrush Warning => ThemeBrushes.Warning;
    private static IBrush Error => ThemeBrushes.Danger;

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
    private static IBrush Waiting => ThemeBrushes.TextSecondary;
    private static IBrush Scanned => ThemeBrushes.Warning;
    private static IBrush Success => ThemeBrushes.Success;
    private static IBrush Error => ThemeBrushes.Danger;

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
