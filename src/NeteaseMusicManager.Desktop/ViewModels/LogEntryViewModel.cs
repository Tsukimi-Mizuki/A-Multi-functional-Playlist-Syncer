namespace NeteaseMusicManager.Desktop.ViewModels;

public enum LogLevel
{
    Info,
    Success,
    Warning,
    Error,
}

/// <summary>底部运行日志中的一行。</summary>
public sealed class LogEntryViewModel
{
    public LogEntryViewModel(LogLevel level, string message)
    {
        Time = DateTime.Now.ToString("HH:mm:ss");
        Level = level;
        Message = message;
    }

    public string Time { get; }
    public LogLevel Level { get; }
    public string Message { get; }

    public string LevelText => Level switch
    {
        LogLevel.Success => "成功",
        LogLevel.Warning => "警告",
        LogLevel.Error => "错误",
        _ => "信息",
    };

    public string LevelKey => Level switch
    {
        LogLevel.Success => "success",
        LogLevel.Warning => "warning",
        LogLevel.Error => "error",
        _ => "info",
    };
}
