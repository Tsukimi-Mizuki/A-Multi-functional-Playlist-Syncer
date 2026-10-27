using CommunityToolkit.Mvvm.ComponentModel;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Core.Online;
using LunaticPrincess.Core.Settings;
using LunaticPrincess.Desktop.Services;

namespace LunaticPrincess.Desktop.ViewModels;

/// <summary>设置对话框：下载/曲库目录、音质、歌词模式、封面、覆盖策略、文件名模板、并发数。</summary>
public sealed partial class SettingsViewModel : ObservableObject
{
    private readonly ISettingsStore _settingsStore;
    // NCM 导入记忆不属于设置界面字段，保存其他设置时必须原样带到新对象，避免丢失。
    private readonly List<string> _ncmImportFolders;
    private readonly string _ncmImportOutputDirectory;
    private readonly OnlineMusicSource _lyricsEditorSearchSource;
    private readonly bool _lyricsEditorPreferWordLyrics;
    private bool _syncingExclusiveFlags;
    private bool _syncingFileNameTemplate;

    public SettingsViewModel(AppSettings current, ISettingsStore settingsStore)
    {
        _settingsStore = settingsStore;
        _ncmImportFolders = current.NcmImportFolders;
        _ncmImportOutputDirectory = current.NcmImportOutputDirectory;
        _lyricsEditorSearchSource = current.LyricsEditorSearchSource;
        _lyricsEditorPreferWordLyrics = current.LyricsEditorPreferWordLyrics;
        _syncingFileNameTemplate = true;

        DownloadDirectory = current.DownloadDirectory;
        LibraryDirectory = current.LibraryDirectory;
        MaxConcurrency = Math.Clamp(current.MaxConcurrency, 1, 8);
        WriteCover = current.WriteCover;
        OverwriteExisting = current.OverwriteExisting;
        SkipExisting = current.SkipExisting;

        QualityOptions =
        [
            new(AudioQuality.Standard, AudioQuality.Standard.ToDisplayName()),
            new(AudioQuality.Higher, AudioQuality.Higher.ToDisplayName()),
            new(AudioQuality.ExHigh, AudioQuality.ExHigh.ToDisplayName()),
            new(AudioQuality.Lossless, AudioQuality.Lossless.ToDisplayName()),
            new(AudioQuality.HiRes, AudioQuality.HiRes.ToDisplayName()),
        ];
        SelectedQuality = QualityOptions.FirstOrDefault(o => o.Value == current.Quality) ?? QualityOptions[2];

        LyricModeOptions =
        [
            new(LyricsWriteMode.None, "不写入歌词", "只下载音频与标签，不写歌词"),
            new(LyricsWriteMode.Line, "逐行歌词（LRC）", "写入普通 LRC 时间轴歌词"),
            new(LyricsWriteMode.Word, "逐字歌词（增强 LRC）", "优先写入网易云逐字歌词，不支持时回退逐行"),
        ];
        SelectedLyricMode = LyricModeOptions.FirstOrDefault(o => o.Value == current.LyricsMode) ?? LyricModeOptions[1];

        ConcurrencyOptions = Enumerable.Range(1, 8).Select(i => new ConcurrencyOption(i, $"{i} 个并发任务")).ToArray();
        SelectedConcurrency = ConcurrencyOptions.FirstOrDefault(o => o.Value == MaxConcurrency) ?? ConcurrencyOptions[2];

        FileNameTemplateOptions =
        [
            new("{artist} - {title}", "作者 - 标题"),
            new("{title} - {artist}", "标题 - 作者"),
            new("{artist} - {title} ({year})", "作者 - 标题（含年份）"),
            FileNameTemplateOption.Custom,
        ];
        FileNameTemplate = current.FileNameTemplate;
        SelectedFileNameTemplate = FileNameTemplateOptions
            .FirstOrDefault(o => o.Value.Length > 0 && string.Equals(o.Value, FileNameTemplate, StringComparison.Ordinal))
            ?? FileNameTemplateOption.Custom;
        _syncingFileNameTemplate = false;

        RenderFrameRateOptions =
        [
            new(RenderFrameRateMode.VSync, "垂直同步（跟随显示器刷新率）"),
            new(RenderFrameRateMode.Fps30, "30 FPS"),
            new(RenderFrameRateMode.Fps60, "60 FPS"),
            new(RenderFrameRateMode.Fps90, "90 FPS"),
            new(RenderFrameRateMode.Fps120, "120 FPS"),
            new(RenderFrameRateMode.Fps144, "144 FPS"),
            new(RenderFrameRateMode.Fps165, "165 FPS"),
            new(RenderFrameRateMode.Fps240, "240 FPS"),
        ];
        SelectedRenderFrameRate = RenderFrameRateOptions
            .FirstOrDefault(o => o.Value == current.RenderFrameRate) ?? RenderFrameRateOptions[0];

        OnlinePriorityOptions =
        [
            new(OnlineSourcePriority.NeteaseFirst, "网易云音乐优先"),
            new(OnlineSourcePriority.QQMusicFirst, "QQ 音乐优先"),
            new(OnlineSourcePriority.NeteaseOnly, "仅网易云音乐"),
            new(OnlineSourcePriority.QQMusicOnly, "仅 QQ 音乐"),
        ];
        SelectedTagSourcePriority = OnlinePriorityOptions
            .FirstOrDefault(o => o.Value == current.TagSourcePriority) ?? OnlinePriorityOptions[0];
        SelectedLyricsSourcePriority = OnlinePriorityOptions
            .FirstOrDefault(o => o.Value == current.LyricsSourcePriority) ?? OnlinePriorityOptions[0];
    }

    public IReadOnlyList<QualityOption> QualityOptions { get; }
    public IReadOnlyList<LyricModeOption> LyricModeOptions { get; }
    public IReadOnlyList<ConcurrencyOption> ConcurrencyOptions { get; }
    public IReadOnlyList<FileNameTemplateOption> FileNameTemplateOptions { get; }
    public IReadOnlyList<RenderFrameRateOption> RenderFrameRateOptions { get; }
    public IReadOnlyList<OnlinePriorityOption> OnlinePriorityOptions { get; }

    [ObservableProperty]
    private string _downloadDirectory = string.Empty;

    [ObservableProperty]
    private string _libraryDirectory = string.Empty;

    [ObservableProperty]
    private string _fileNameTemplate = string.Empty;

    [ObservableProperty]
    private bool _writeCover;

    [ObservableProperty]
    private bool _overwriteExisting;

    [ObservableProperty]
    private bool _skipExisting;

    [ObservableProperty]
    private int _maxConcurrency = 3;

    [ObservableProperty]
    private QualityOption _selectedQuality = null!;

    [ObservableProperty]
    private LyricModeOption _selectedLyricMode = null!;

    [ObservableProperty]
    private ConcurrencyOption _selectedConcurrency = null!;

    [ObservableProperty]
    private FileNameTemplateOption _selectedFileNameTemplate = null!;

    [ObservableProperty]
    private RenderFrameRateOption _selectedRenderFrameRate = null!;

    [ObservableProperty]
    private OnlinePriorityOption _selectedTagSourcePriority = null!;

    [ObservableProperty]
    private OnlinePriorityOption _selectedLyricsSourcePriority = null!;

    [ObservableProperty]
    private string? _validationMessage;

    [ObservableProperty]
    private bool _isSaving;

    public bool HasValidationMessage => !string.IsNullOrWhiteSpace(ValidationMessage);

    partial void OnValidationMessageChanged(string? value) => OnPropertyChanged(nameof(HasValidationMessage));

    partial void OnSelectedConcurrencyChanged(ConcurrencyOption value) => MaxConcurrency = value.Value;

    partial void OnSelectedFileNameTemplateChanged(FileNameTemplateOption value)
    {
        if (_syncingFileNameTemplate || value is null || value.Value.Length == 0)
        {
            return;
        }

        _syncingFileNameTemplate = true;
        FileNameTemplate = value.Value;
        _syncingFileNameTemplate = false;
    }

    partial void OnFileNameTemplateChanged(string value)
    {
        if (_syncingFileNameTemplate || FileNameTemplateOptions is null)
        {
            return;
        }

        var match = FileNameTemplateOptions.FirstOrDefault(
            option => option.Value.Length > 0 &&
                      string.Equals(option.Value, value, StringComparison.Ordinal));
        _syncingFileNameTemplate = true;
        SelectedFileNameTemplate = match ?? FileNameTemplateOption.Custom;
        _syncingFileNameTemplate = false;
    }

    partial void OnOverwriteExistingChanged(bool value)
    {
        if (!value || _syncingExclusiveFlags)
        {
            return;
        }

        _syncingExclusiveFlags = true;
        SkipExisting = false;
        _syncingExclusiveFlags = false;
    }

    partial void OnSkipExistingChanged(bool value)
    {
        if (!value || _syncingExclusiveFlags)
        {
            return;
        }

        _syncingExclusiveFlags = true;
        OverwriteExisting = false;
        _syncingExclusiveFlags = false;
    }

    /// <summary>校验并持久化设置；失败时把错误写入 <see cref="ValidationMessage"/> 并返回 null。</summary>
    public async Task<AppSettings?> SaveAsync(CancellationToken cancellationToken = default)
    {
        if (IsSaving)
        {
            return null;
        }

        ValidationMessage = null;
        var download = DownloadDirectory?.Trim() ?? string.Empty;
        var library = LibraryDirectory?.Trim() ?? string.Empty;
        var template = FileNameTemplate?.Trim() ?? string.Empty;

        if (download.Length == 0)
        {
            ValidationMessage = "下载目录不能为空";
            return null;
        }

        if (!Path.IsPathFullyQualified(download))
        {
            ValidationMessage = "下载目录必须是完整路径（例如 D:\\Music\\网易云）";
            return null;
        }

        if (library.Length == 0)
        {
            ValidationMessage = "曲库目录不能为空";
            return null;
        }

        if (!Path.IsPathFullyQualified(library))
        {
            ValidationMessage = "曲库目录必须是完整路径（例如 D:\\Music\\网易云）";
            return null;
        }

        if (template.Length == 0)
        {
            ValidationMessage = "文件名模板不能为空";
            return null;
        }

        if (template.IndexOfAny(Path.GetInvalidFileNameChars()) >= 0)
        {
            ValidationMessage = "文件名模板包含非法字符（如 : \\ / * ? \" < > |）";
            return null;
        }

        IsSaving = true;
        try
        {
            try
            {
                Directory.CreateDirectory(download);
                Directory.CreateDirectory(library);
            }
            catch (Exception ex)
            {
                ValidationMessage = "目录不可用：" + ErrorFormatter.ToFriendlyMessage(ex);
                return null;
            }

            var settings = new AppSettings
            {
                DownloadDirectory = download,
                LibraryDirectory = library,
                Quality = SelectedQuality.Value,
                LyricsMode = SelectedLyricMode.Value,
                WriteCover = WriteCover,
                OverwriteExisting = OverwriteExisting,
                SkipExisting = SkipExisting || !OverwriteExisting,
                FileNameTemplate = template,
                MaxConcurrency = Math.Clamp(SelectedConcurrency.Value, 1, 8),
                RenderFrameRate = SelectedRenderFrameRate.Value,
                TagSourcePriority = SelectedTagSourcePriority.Value,
                LyricsSourcePriority = SelectedLyricsSourcePriority.Value,
                NcmImportFolders = new List<string>(_ncmImportFolders),
                NcmImportOutputDirectory = _ncmImportOutputDirectory,
                LyricsEditorSearchSource = _lyricsEditorSearchSource,
                LyricsEditorPreferWordLyrics = _lyricsEditorPreferWordLyrics,
            };

            await _settingsStore.SaveAsync(settings, cancellationToken).ConfigureAwait(true);
            return settings;
        }
        catch (OperationCanceledException)
        {
            ValidationMessage = "保存已取消";
            return null;
        }
        catch (Exception ex)
        {
            ValidationMessage = "保存设置失败：" + ErrorFormatter.ToFriendlyMessage(ex);
            return null;
        }
        finally
        {
            IsSaving = false;
        }
    }
}
