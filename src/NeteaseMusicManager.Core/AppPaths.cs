namespace NeteaseMusicManager.Core;

/// <summary>Well-known locations used by the desktop app. The interface layer may override them in tests.</summary>
public static class AppPaths
{
    public const string AppName = "NeteaseMusicManager";

    public static string DataDirectory =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), AppName);

    public static string SettingsFilePath => Path.Combine(DataDirectory, "settings.json");
    public static string SessionFilePath => Path.Combine(DataDirectory, "session.dat");
    public static string LogDirectory => Path.Combine(DataDirectory, "logs");

    public static string DefaultDownloadDirectory =>
        Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyMusic), AppName);

    public static void EnsureDataDirectory() => Directory.CreateDirectory(DataDirectory);
}
