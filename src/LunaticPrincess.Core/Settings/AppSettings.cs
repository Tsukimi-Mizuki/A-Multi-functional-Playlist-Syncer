using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Online;

namespace LunaticPrincess.Core.Settings;

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
    /// <summary>NCM 导入窗口记住的输入文件夹列表（按添加顺序，自动去重）。</summary>
    public List<string> NcmImportFolders { get; set; } = [];
    /// <summary>NCM 导入窗口记住的输出目录；为空时回退到下载目录。</summary>
    public string NcmImportOutputDirectory { get; set; } = string.Empty;
    /// <summary>歌词编辑窗口记住的在线搜索来源。</summary>
    public OnlineMusicSource LyricsEditorSearchSource { get; set; } = OnlineMusicSource.Netease;
    /// <summary>歌词编辑窗口记住的在线歌词格式：true 表示优先逐字歌词。</summary>
    public bool LyricsEditorPreferWordLyrics { get; set; }
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

        if (!Enum.IsDefined(typeof(OnlineMusicSource), LyricsEditorSearchSource))
        {
            LyricsEditorSearchSource = OnlineMusicSource.Netease;
        }

        NcmImportOutputDirectory = (NcmImportOutputDirectory ?? string.Empty).Trim();
        var rememberedFolders = new List<string>();
        var seenFolders = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var folder in NcmImportFolders ?? [])
        {
            var trimmed = folder?.Trim();
            if (!string.IsNullOrWhiteSpace(trimmed) && seenFolders.Add(trimmed))
            {
                rememberedFolders.Add(trimmed);
            }
        }

        NcmImportFolders = rememberedFolders;

        return this;
    }
}
