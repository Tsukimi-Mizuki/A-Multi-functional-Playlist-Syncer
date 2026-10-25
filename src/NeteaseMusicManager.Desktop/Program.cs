using System.Text;
using Avalonia;
using NeteaseMusicManager.Core;

namespace NeteaseMusicManager.Desktop;

internal static class Program
{
    [STAThread]
    public static void Main(string[] args)
    {
        AppDomain.CurrentDomain.UnhandledException += (_, e) =>
            LogFatal("AppDomain", e.ExceptionObject as Exception);
        TaskScheduler.UnobservedTaskException += (_, e) =>
        {
            LogFatal("UnobservedTask", e.Exception);
            e.SetObserved();
        };

        try
        {
            BuildAvaloniaApp().StartWithClassicDesktopLifetime(args);
        }
        catch (Exception ex)
        {
            LogFatal("Startup", ex);
            throw;
        }
    }

    public static AppBuilder BuildAvaloniaApp() =>
        AppBuilder.Configure<App>()
            .UsePlatformDetect()
            .WithInterFont()
            .LogToTrace();

    /// <summary>把致命异常追加写入日志目录，便于用户汇报问题时定位（失败时静默）。</summary>
    private static void LogFatal(string source, Exception? exception)
    {
        try
        {
            Directory.CreateDirectory(AppPaths.LogDirectory);
            var path = Path.Combine(AppPaths.LogDirectory, "desktop-fatal.log");
            var builder = new StringBuilder();
            builder.Append('[').Append(DateTimeOffset.Now.ToString("yyyy-MM-dd HH:mm:ss")).Append("] ")
                .Append(source).Append(": ").AppendLine(exception?.ToString() ?? "(no exception object)");
            File.AppendAllText(path, builder.ToString());
        }
        catch (Exception)
        {
            // 日志失败不应掩盖原始异常。
        }
    }
}
