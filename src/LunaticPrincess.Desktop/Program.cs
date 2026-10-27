using System.Text;
using Avalonia;
using LunaticPrincess.Core;
using LunaticPrincess.Desktop.Services;

namespace LunaticPrincess.Desktop;

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
            .LogToTrace()
            .AfterSetup(_ => RenderTimerInstaller.InstallFromSettings());

    /// <summary>支持通过 --lyrics/--edit 参数启动后直接打开歌词编辑器。</summary>
    public static string? GetStartupLyricsFile()
    {
        var args = Environment.GetCommandLineArgs();
        for (var i = 1; i < args.Length - 1; i++)
        {
            if (args[i] is "--lyrics" or "--edit" &&
                !string.IsNullOrWhiteSpace(args[i + 1]) &&
                File.Exists(args[i + 1]))
            {
                return Path.GetFullPath(args[i + 1]);
            }
        }

        return null;
    }

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
