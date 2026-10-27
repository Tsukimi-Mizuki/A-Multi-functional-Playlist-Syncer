namespace LunaticPrincess.Core;

/// <summary>
/// Well-known locations used by the desktop app.
/// <para>
/// 某些受限运行环境（企业策略/沙箱/精简用户配置）不允许进程写入
/// <c>%LOCALAPPDATA%</c> 或“音乐”目录。为避免登录 Cookie、设置与下载全部失败，
/// 这里会先探测标准目录是否可写；不可写时自动降级到程序目录下的 data（便携模式），
/// 再退到临时目录。便携模式一旦启用（程序目录下存在 data 目录）会保持稳定，
/// 避免同一份程序在不同启动方式下使用不同登录态。正常 Windows 用户始终使用标准 AppData/Music 路径。
/// </para>
/// </summary>
public static class AppPaths
{
    public const string AppName = "LunaticPrincess";

    /// <summary>更名前旧版本的目录名，用于自动迁移设置、登录态与默认下载目录。</summary>
    public const string LegacyAppName = "NeteaseMusicManager";

    /// <summary>可通过该环境变量强制指定数据目录（测试/便携部署）。</summary>
    public const string DataDirectoryEnvironmentVariable = "LUNATIC_PRINCESS_DATA_DIR";

    /// <summary>更名前的同名环境变量，保留兼容（新变量未设置时仍然生效）。</summary>
    public const string LegacyDataDirectoryEnvironmentVariable = "NETEASE_MUSIC_MANAGER_DATA_DIR";

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
        if (string.IsNullOrWhiteSpace(overridePath))
        {
            overridePath = Environment.GetEnvironmentVariable(LegacyDataDirectoryEnvironmentVariable);
        }

        if (!string.IsNullOrWhiteSpace(overridePath) && TryPrepareDirectory(overridePath))
        {
            return Path.GetFullPath(overridePath);
        }

        // 已经启用便携模式的安装：继续使用程序目录下的 data，
        // 这样无论通过资源管理器还是受限宿主启动，登录态/设置位置都保持一致。
        var portable = Path.Combine(AppContext.BaseDirectory, "data");
        if (Directory.Exists(portable) && TryPrepareDirectory(portable))
        {
            return portable;
        }

        var localAppData = Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData);
        if (!string.IsNullOrWhiteSpace(localAppData))
        {
            var standard = Path.Combine(localAppData, AppName);
            var legacy = Path.Combine(localAppData, LegacyAppName);
            if (!Directory.Exists(standard) && Directory.Exists(legacy))
            {
                TryMoveLegacyDirectory(legacy, standard);
            }

            if (TryPrepareDirectory(standard))
            {
                return standard;
            }
        }

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
        // 便携模式下下载目录也跟随程序目录，避免换一种启动方式后默认路径不可写。
        if (IsPortableDataDirectory())
        {
            var portableMusic = Path.Combine(DataDirectory, "Music");
            if (TryPrepareDirectory(portableMusic))
            {
                return portableMusic;
            }
        }

        var music = Environment.GetFolderPath(Environment.SpecialFolder.MyMusic);
        if (!string.IsNullOrWhiteSpace(music))
        {
            var standard = Path.Combine(music, AppName);
            var legacy = Path.Combine(music, LegacyAppName);

            // 旧版默认下载目录里已有文件时沿用旧目录，避免用户已下载的内容被拆成两处。
            if (!Directory.Exists(standard) && Directory.Exists(legacy))
            {
                return legacy;
            }

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

    /// <summary>把更名前的数据目录整体迁移到新目录；失败时保持原样子继续使用新目录。</summary>
    private static void TryMoveLegacyDirectory(string legacy, string target)
    {
        try
        {
            Directory.Move(legacy, target);
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException or NotSupportedException)
        {
            // 旧目录可能被占用或没有权限：不阻断启动，由调用方继续探测新目录是否可写。
        }
    }

    private static bool IsPortableDataDirectory()
    {
        try
        {
            var baseDir = Path.GetFullPath(AppContext.BaseDirectory)
                .TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar) + Path.DirectorySeparatorChar;
            var dataDir = Path.GetFullPath(DataDirectory);
            return dataDir.StartsWith(baseDir, StringComparison.OrdinalIgnoreCase);
        }
        catch (Exception ex) when (ex is IOException or ArgumentException or NotSupportedException)
        {
            return false;
        }
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
