using LunaticPrincess.Core.Lyrics;
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
    /// <summary>歌词编辑窗口记住的在线搜索来源（主词源或备用词源）。</summary>
    public LyricsSourceKind LyricsEditorSearchSource { get; set; } = LyricsSourceKind.Netease;
    /// <summary>批量获取歌词时是否启用酷狗音乐备用词源（逐字 + 译文）。</summary>
    public bool FallbackKugouLyrics { get; set; } = true;
    /// <summary>批量获取歌词时是否启用 LRCLIB 备用词源（逐行）。</summary>
    public bool FallbackLrclibLyrics { get; set; } = true;
    /// <summary>歌词编辑窗口记住的在线歌词格式：true 表示优先逐字歌词。</summary>
    public bool LyricsEditorPreferWordLyrics { get; set; }
    /// <summary>在线匹配度阈值（0.30-1.00）：同时影响歌词编辑器搜索排序与批量获取的精度。</summary>
    public double MatchThreshold { get; set; } = LyricsMatchScorer.DefaultMinScore;
    /// <summary>界面主题：深色/浅色液态玻璃，或跟随系统。</summary>
    public ThemeMode Theme { get; set; } = ThemeMode.Dark;
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
        if (!Enum.IsDefined(typeof(AudioQuality), Quality))
        {
            Quality = AudioQuality.ExHigh;
        }

        if (!Enum.IsDefined(typeof(ThemeMode), Theme))
        {
            Theme = ThemeMode.Dark;
        }

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

        if (!Enum.IsDefined(typeof(LyricsSourceKind), LyricsEditorSearchSource))
        {
            LyricsEditorSearchSource = LyricsSourceKind.Netease;
        }

        if (double.IsNaN(MatchThreshold) || double.IsInfinity(MatchThreshold) || MatchThreshold <= 0)
        {
            MatchThreshold = LyricsMatchScorer.DefaultMinScore;
        }

        MatchThreshold = Math.Clamp(
            Math.Round(MatchThreshold, 2),
            LyricsMatchScorer.MinThreshold,
            LyricsMatchScorer.MaxThreshold);

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
