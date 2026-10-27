using System.Diagnostics;
using System.Reflection;
using System.Runtime.InteropServices;
using Avalonia;
using LunaticPrincess.Core;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Settings;

namespace LunaticPrincess.Desktop.Services;

/// <summary>
/// Avalonia 11.3 的 Win32 渲染计时器要么是 WinUI Composition（原生垂直同步），
/// 要么是固定 60FPS 的 DefaultRenderTimer。这里在平台初始化后接管 Tick：
/// 垂直同步使用原生/ DWM 刷新；固定 FPS 模式只按指定间隔放行 Tick。
/// 设置修改后需重启应用生效。
/// </summary>
internal static class RenderTimerInstaller
{
    public static RenderFrameRateMode? InstalledMode { get; private set; }

    public static void InstallFromSettings()
    {
        var mode = RenderFrameRateMode.VSync;
        try
        {
            mode = new JsonSettingsStore().LoadAsync().GetAwaiter().GetResult().RenderFrameRate;
        }
        catch (Exception)
        {
            // 读取失败时使用垂直同步默认值。
        }

        Install(mode);
    }

    public static void Install(RenderFrameRateMode mode)
    {
        try
        {
            var timer = ResolveRenderTimer();
            if (timer is null)
            {
                Debug.WriteLine("Avalonia render timer not found; keeping default 60FPS.");
                WriteDiagnostic("timer not found");
                return;
            }

            var branch = InstallForTimer(timer, mode);
            InstalledMode = mode;
            WriteDiagnostic($"mode={mode} timer={timer.GetType().FullName} branch={branch}");
        }
        catch (Exception ex)
        {
            Debug.WriteLine($"Render timer install failed: {ex}");
            WriteDiagnostic($"failed: {ex}");
        }
    }

    private static string InstallForTimer(object timer, RenderFrameRateMode mode)
    {
        // WinUI Composition 计时器：事件字段名就是 Tick，本身已经跟随 DWM 垂直同步。
        var eventField = FindField(timer.GetType(), "Tick");
        if (eventField?.FieldType == typeof(Action<TimeSpan>) &&
            eventField.GetValue(timer) is Action<TimeSpan> eventTick &&
            eventTick.GetInvocationList().Length > 0)
        {
            if (mode == RenderFrameRateMode.VSync)
            {
                return "winui-native-vsync";
            }

            eventField.SetValue(timer, Throttle(eventTick, (int)mode));
            return "winui-throttle";
        }

        // 传统 DefaultRenderTimer：内部 60FPS 的 System.Threading.Timer 直接替换成自驱动线程。
        var tickField = FindField(timer.GetType(), "_tick");
        if (tickField?.GetValue(timer) is not Action<TimeSpan> tick || tick.GetInvocationList().Length == 0)
        {
            return "no-tick-subscribers";
        }

        var subscriptionField = FindField(timer.GetType(), "_subscription");
        (subscriptionField?.GetValue(timer) as IDisposable)?.Dispose();
        subscriptionField?.SetValue(timer, null);

        var clock = Stopwatch.StartNew();
        var thread = new Thread(() =>
        {
            if (mode == RenderFrameRateMode.VSync)
            {
                RunVSync(tick, clock);
            }
            else
            {
                RunFixed((int)mode, tick, clock);
            }
        })
        {
            IsBackground = true,
            Name = "LunaticPrincess.RenderTimer",
        };
        thread.Start();
        return mode == RenderFrameRateMode.VSync ? "dwm-vsync" : "fixed-fps";
    }

    private static Action<TimeSpan> Throttle(Action<TimeSpan> inner, int fps)
    {
        var interval = TimeSpan.FromSeconds(1.0 / Math.Max(1, fps));
        var clock = Stopwatch.StartNew();
        var gate = new object();
        var next = TimeSpan.Zero;

        return time =>
        {
            lock (gate)
            {
                var now = clock.Elapsed;
                if (now < next)
                {
                    return;
                }

                next = now + interval;
                inner(time);
            }
        };
    }

    private static void RunVSync(Action<TimeSpan> tick, Stopwatch clock)
    {
        while (true)
        {
            var result = DwmFlush();
            if (result != 0)
            {
                // 远程桌面/关闭 DWM 时退回约 60Hz，避免忙循环。
                Thread.Sleep(16);
            }

            tick(clock.Elapsed);
        }
    }

    private static void RunFixed(int fps, Action<TimeSpan> tick, Stopwatch clock)
    {
        var interval = TimeSpan.FromSeconds(1.0 / Math.Max(1, fps));
        var next = clock.Elapsed;

        while (true)
        {
            tick(clock.Elapsed);

            next += interval;
            var wait = next - clock.Elapsed;
            if (wait > TimeSpan.FromMilliseconds(1))
            {
                Thread.Sleep(wait);
            }
            else if (wait > TimeSpan.Zero)
            {
                Thread.SpinWait(200);
            }

            if (clock.Elapsed - next > interval)
            {
                next = clock.Elapsed;
            }
        }
    }

    private static object? ResolveRenderTimer()
    {
        var locatorType = typeof(AvaloniaLocator);
        var current = locatorType.GetProperty("Current", BindingFlags.Public | BindingFlags.Static)?.GetValue(null);
        var timerType = locatorType.Assembly.GetType("Avalonia.Rendering.IRenderTimer");
        if (current is null || timerType is null)
        {
            return null;
        }

        var getService = locatorType.GetMethod(
            "GetService",
            BindingFlags.Instance | BindingFlags.Public,
            null,
            new[] { typeof(Type) },
            null);
        return getService?.Invoke(current, new object?[] { timerType });
    }

    private static FieldInfo? FindField(Type type, string name)
    {
        for (var current = type; current is not null; current = current.BaseType)
        {
            var field = current.GetField(name, BindingFlags.Instance | BindingFlags.NonPublic | BindingFlags.Public);
            if (field is not null)
            {
                return field;
            }
        }

        return null;
    }

    private static void WriteDiagnostic(string message)
    {
        if (!string.Equals(Environment.GetEnvironmentVariable("NMM_RENDER_TIMER_LOG"), "1", StringComparison.Ordinal))
        {
            return;
        }

        try
        {
            Directory.CreateDirectory(AppPaths.LogDirectory);
            File.AppendAllText(
                Path.Combine(AppPaths.LogDirectory, "render-timer.log"),
                $"[{DateTimeOffset.Now:O}] {message}{Environment.NewLine}");
        }
        catch (Exception)
        {
            // 诊断日志失败不影响启动。
        }
    }

    [DllImport("dwmapi.dll")]
    private static extern int DwmFlush();
}
