namespace NeteaseMusicManager.Core;

/// <summary>
/// Well-known locations used by the desktop app.
/// <para>
/// 某些受限运行环境（企业策略/沙箱/精简用户配置）不允许进程写入
/// <c>%LOCALAPPDATA%</c> 或“音乐”目录。为避免登录 Cookie、设置与下载全部失败，
/// 这里会先探测标准目录是否可写；不可写时自动降级到程序目录下的 data（便携模式），
/// 再退到临时目录。正常 Windows 用户始终使用标准 AppData/Music 路径。
/// </para>
/// </summary>
public static class AppPaths
{
    public const string AppName = "NeteaseMusicManager";

    /// <summary>可通过该环境变量强制指定数据目录（测试/便携部署）。</summary>
    public const string DataDirectoryEnvironmentVariable = "NETEASE_MUSIC_MANAGER_DATA_DIR";

    private static readonly string ResolvedDataDirectory = ResolveDataDirectory();
    private static readonly string ResolvedDownloadDirectory = ResolveDownloadDirectory();

    public static string DataDirectory => ResolvedDataDirectory;

    public static string SettingsFilePath => Path.Combine(DataDirectory, "settings.json");
    public static string SessionFilePath => Path.Combine(DataDirectory, "session.dat");
    public static string LogDirectory => Path.Combine(DataDirectory, "logs");

    public static string DefaultDownloadDirectory => ResolvedDownloadDirectory;

    public static void EnsureDataDirectory() => Directory.CreateDirectory(DataDirectory);

    private static string ResolveDataDirectory()
    {
        var overridePath = Environment.GetEnvironmentVariable(DataDirectoryEnvironmentVariable);
        if (!string.IsNullOrWhiteSpace(overridePath) && TryPrepareDirectory(overridePath))
        {
            return Path.GetFullPath(overridePath);
        }

        var localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        if (!string.IsNullOrWhiteSpace(localAppData))
        {
            var standard = Path.Combine(localAppData, AppName);
            if (TryPrepareDirectory(standard))
            {
                return standard;
            }
        }

        var portable = Path.Combine(AppContext.BaseDirectory, "data");
        if (TryPrepareDirectory(portable))
        {
            return portable;
        }

        var fallback = Path.Combine(Path.GetTempPath(), AppName);
        Directory.CreateDirectory(fallback);
        return fallback;
    }

    private static string ResolveDownloadDirectory()
    {
        var music = Environment.GetFolderPath(Environment.SpecialFolder.MyMusic);
        if (!string.IsNullOrWhiteSpace(music))
        {
            var standard = Path.Combine(music, AppName);
            if (TryPrepareDirectory(standard))
            {
                return standard;
            }
        }

        var portable = Path.Combine(DataDirectory, "Music");
        if (TryPrepareDirectory(portable))
        {
            return portable;
        }

        return Path.Combine(Path.GetTempPath(), AppName, "Music");
    }

    /// <summary>尝试创建目录并做一次真实写入探测（某些容器对目录有特殊 ACL）。</summary>
    private static bool TryPrepareDirectory(string path)
    {
        try
        {
            if (string.IsNullOrWhiteSpace(path))
            {
                return false;
            }

            Directory.CreateDirectory(path);
            var probe = Path.Combine(path, ".write-probe-" + Guid.NewGuid().ToString("N"));
            using var stream = new FileStream(
                probe,
                FileMode.CreateNew,
                FileAccess.Write,
                FileShare.None,
                bufferSize: 1,
                FileOptions.DeleteOnClose);
            stream.WriteByte(0x42);
            return true;
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException or NotSupportedException)
        {
            return false;
        }
    }
}
