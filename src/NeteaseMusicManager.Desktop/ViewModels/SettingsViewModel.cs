using CommunityToolkit.Mvvm.ComponentModel;
using NeteaseMusicManager.Core.Models;
using NeteaseMusicManager.Core.Settings;
using NeteaseMusicManager.Desktop.Services;

namespace NeteaseMusicManager.Desktop.ViewModels;

/// <summary>设置对话框：下载/曲库目录、音质、歌词模式、封面、覆盖策略、文件名模板、并发数。</summary>
public sealed partial class SettingsViewModel : ObservableObject
{
    private readonly ISettingsStore _settingsStore;
    private bool _syncingExclusiveFlags;

    public SettingsViewModel(AppSettings current, ISettingsStore settingsStore)
    {
        _settingsStore = settingsStore;

        DownloadDirectory = current.DownloadDirectory;
        LibraryDirectory = current.LibraryDirectory;
        FileNameTemplate = current.FileNameTemplate;
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
    }

    public IReadOnlyList<QualityOption> QualityOptions { get; }
    public IReadOnlyList<LyricModeOption> LyricModeOptions { get; }
    public IReadOnlyList<ConcurrencyOption> ConcurrencyOptions { get; }

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
    private string? _validationMessage;

    [ObservableProperty]
    private bool _isSaving;

    public bool HasValidationMessage => !string.IsNullOrWhiteSpace(ValidationMessage);

    partial void OnValidationMessageChanged(string? value) => OnPropertyChanged(nameof(HasValidationMessage));

    partial void OnSelectedConcurrencyChanged(ConcurrencyOption value) => MaxConcurrency = value.Value;

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
