using NeteaseMusicManager.Core.Models;

namespace NeteaseMusicManager.Core.Settings;

public sealed class AppSettings
{
    public string DownloadDirectory { get; set; } = AppPaths.DefaultDownloadDirectory;
    public string LibraryDirectory { get; set; } = AppPaths.DefaultDownloadDirectory;
    public AudioQuality Quality { get; set; } = AudioQuality.ExHigh;
    public LyricsWriteMode LyricsMode { get; set; } = LyricsWriteMode.Line;
    public bool WriteCover { get; set; } = true;
    public bool OverwriteExisting { get; set; }
    public bool SkipExisting { get; set; } = true;
    public string FileNameTemplate { get; set; } = "{artist} - {title}";
    public int MaxConcurrency { get; set; } = 3;
    public bool AutoMatchLocal { get; set; } = true;
    public OnlineSourcePriority TagSourcePriority { get; set; } = OnlineSourcePriority.NeteaseFirst;
    public OnlineSourcePriority LyricsSourcePriority { get; set; } = OnlineSourcePriority.NeteaseFirst;
    public RenderFrameRateMode RenderFrameRate { get; set; } = RenderFrameRateMode.VSync;

    public AppSettings Clone() => (AppSettings)MemberwiseClone();

    public AppSettings Normalize()
    {
        DownloadDirectory = string.IsNullOrWhiteSpace(DownloadDirectory) ? AppPaths.DefaultDownloadDirectory : DownloadDirectory.Trim();
        LibraryDirectory = string.IsNullOrWhiteSpace(LibraryDirectory) ? DownloadDirectory : LibraryDirectory.Trim();
        if (string.IsNullOrWhiteSpace(FileNameTemplate))
        {
            FileNameTemplate = "{artist} - {title}";
        }

        MaxConcurrency = Math.Clamp(MaxConcurrency, 1, 8);
        if (!Enum.IsDefined(typeof(RenderFrameRateMode), RenderFrameRate))
        {
            RenderFrameRate = RenderFrameRateMode.VSync;
        }

        if (!Enum.IsDefined(typeof(OnlineSourcePriority), TagSourcePriority))
        {
            TagSourcePriority = OnlineSourcePriority.NeteaseFirst;
        }

        if (!Enum.IsDefined(typeof(OnlineSourcePriority), LyricsSourcePriority))
        {
            LyricsSourcePriority = OnlineSourcePriority.NeteaseFirst;
        }

        return this;
    }
}
