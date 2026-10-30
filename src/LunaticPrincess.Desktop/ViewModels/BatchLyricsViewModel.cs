using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using LunaticPrincess.Core.Lyrics;
using LunaticPrincess.Core.Models;
using LunaticPrincess.Desktop.Services;

namespace LunaticPrincess.Desktop.ViewModels;

public sealed partial class BatchLyricsViewModel : ObservableObject, IDisposable
{
    private readonly IBatchLyricsService _batch;
    private readonly IDesktopUiService _ui;
    private readonly OnlineSourcePriority _tagPriority;
    private readonly OnlineSourcePriority _lyricsPriority;
    private readonly double _minScore;
    private CancellationTokenSource? _cts;

    public BatchLyricsViewModel(
        IBatchLyricsService batch,
        IDesktopUiService ui,
        string? initialDirectory,
        OnlineSourcePriority tagPriority,
        OnlineSourcePriority lyricsPriority,
        double minScore = LyricsMatchScorer.DefaultMinScore)
    {
        _batch = batch;
        _ui = ui;
        _tagPriority = tagPriority;
        _lyricsPriority = lyricsPriority;
        _minScore = Math.Clamp(
            Math.Round(minScore, 2),
            LyricsMatchScorer.MinThreshold,
            LyricsMatchScorer.MaxThreshold);
        FolderPath = Directory.Exists(initialDirectory) ? initialDirectory! : string.Empty;
        TagPriorityText = PriorityLabel(tagPriority);
        LyricsPriorityText = PriorityLabel(lyricsPriority);
        MatchThresholdText = $"匹配度阈值 {_minScore * 100:0}%（在「设置」的「在线匹配度」里调整，低于该值的歌曲会被跳过）";
        LyricModeOptions =
        [
            new(false, "逐行歌词"),
            new(true, "逐字歌词（有则优先）"),
        ];
        _selectedLyricMode = LyricModeOptions[0];
    }

    public ObservableCollection<BatchLyricsItemViewModel> Items { get; } = [];

    public IReadOnlyList<OnlineLyricModeOption> LyricModeOptions { get; }

    public string TagPriorityText { get; }
    public string LyricsPriorityText { get; }
    public string MatchThresholdText { get; }

    [ObservableProperty]
    private string _folderPath = string.Empty;

    [ObservableProperty]
    private bool _fetchLyrics = true;

    [ObservableProperty]
    private OnlineLyricModeOption _selectedLyricMode;

    [ObservableProperty]
    private bool _overwriteExisting = true;

    [ObservableProperty]
    private bool _recursive = true;

    [ObservableProperty]
    private bool _isRunning;

    [ObservableProperty]
    private string _statusText = "选择目录后开始批量获取标签、封面与歌词。";

    [ObservableProperty]
    private double _progressPercent;

    [ObservableProperty]
    private int _totalCount;

    [ObservableProperty]
    private int _processedCount;

    [ObservableProperty]
    private int _successCount;

    [ObservableProperty]
    private int _skippedCount;

    [ObservableProperty]
    private int _failedCount;

    public string ProgressText =>
        $"已处理 {ProcessedCount}/{TotalCount} · 成功 {SuccessCount} · 跳过 {SkippedCount} · 失败 {FailedCount}";

    public bool CanStart => !IsRunning && !string.IsNullOrWhiteSpace(FolderPath);

    partial void OnIsRunningChanged(bool value) => OnPropertyChanged(nameof(CanStart));

    partial void OnFolderPathChanged(string value) => OnPropertyChanged(nameof(CanStart));

    partial void OnProcessedCountChanged(int value) => OnPropertyChanged(nameof(ProgressText));

    partial void OnTotalCountChanged(int value) => OnPropertyChanged(nameof(ProgressText));

    partial void OnSuccessCountChanged(int value) => OnPropertyChanged(nameof(ProgressText));

    partial void OnSkippedCountChanged(int value) => OnPropertyChanged(nameof(ProgressText));

    partial void OnFailedCountChanged(int value) => OnPropertyChanged(nameof(ProgressText));

    [RelayCommand]
    private async Task PickFolderAsync()
    {
        try
        {
            var folder = await _ui.PickFolderAsync("选择要批量匹配标签与封面的目录").ConfigureAwait(true);
            if (!string.IsNullOrWhiteSpace(folder))
            {
                FolderPath = folder;
            }
        }
        catch (Exception ex)
        {
            StatusText = "选择目录失败：" + ErrorFormatter.ToFriendlyMessage(ex);
        }
    }

    [RelayCommand]
    private async Task StartAsync()
    {
        if (IsRunning)
        {
            return;
        }

        if (string.IsNullOrWhiteSpace(FolderPath) || !Directory.Exists(FolderPath))
        {
            StatusText = "请选择有效的音乐目录。";
            return;
        }

        IsRunning = true;
        Items.Clear();
        TotalCount = 0;
        ProcessedCount = 0;
        SuccessCount = 0;
        SkippedCount = 0;
        FailedCount = 0;
        ProgressPercent = 0;
        _cts = new CancellationTokenSource();
        var token = _cts.Token;

        try
        {
            StatusText = "正在扫描音频文件...";
            var files = await _batch.ScanAudioFilesAsync(FolderPath, Recursive, token).ConfigureAwait(true);
            TotalCount = files.Count;
            if (files.Count == 0)
            {
                StatusText = "目录中没有找到支持的音频文件。";
                return;
            }

            var itemsByPath = new Dictionary<string, BatchLyricsItemViewModel>(StringComparer.OrdinalIgnoreCase);
            foreach (var file in files)
            {
                var item = new BatchLyricsItemViewModel(file);
                Items.Add(item);
                itemsByPath[file] = item;
            }

            var options = new BatchLyricsOptions(
                _tagPriority,
                _lyricsPriority,
                OverwriteExisting,
                FetchLyrics,
                SelectedLyricMode.PreferWord,
                _minScore);

            IProgress<BatchLyricsResult> progress = new Progress<BatchLyricsResult>(result =>
            {
                if (!itemsByPath.TryGetValue(result.FilePath, out var item))
                {
                    return;
                }

                item.Apply(result);
                ProcessedCount++;
                if (result.Success)
                {
                    SuccessCount++;
                }
                else if (result.Skipped)
                {
                    SkippedCount++;
                }
                else
                {
                    FailedCount++;
                }

                ProgressPercent = TotalCount > 0 ? ProcessedCount * 100.0 / TotalCount : 0;
            });

            using var semaphore = new SemaphoreSlim(3, 3);
            var tasks = files.Select(async file =>
            {
                await semaphore.WaitAsync(token).ConfigureAwait(true);
                try
                {
                    var result = await _batch.FetchForFileAsync(file, options, token).ConfigureAwait(true);
                    progress.Report(result);
                }
                catch (OperationCanceledException)
                {
                    // 取消时无需逐项记录。
                }
                catch (Exception ex)
                {
                    progress.Report(new BatchLyricsResult(file, false, false, "处理失败：" + ErrorFormatter.ToFriendlyMessage(ex)));
                }
                finally
                {
                    semaphore.Release();
                }
            });

            StatusText = "正在批量获取标签、封面与歌词...";
            await Task.WhenAll(tasks).ConfigureAwait(true);
            StatusText = token.IsCancellationRequested
                ? "已取消批量标签获取。"
                : $"完成：成功 {SuccessCount}，跳过 {SkippedCount}，失败 {FailedCount}。";
        }
        catch (OperationCanceledException)
        {
            StatusText = "已取消批量标签获取。";
        }
        catch (Exception ex)
        {
            StatusText = "批量标签获取失败：" + ErrorFormatter.ToFriendlyMessage(ex);
        }
        finally
        {
            _cts?.Dispose();
            _cts = null;
            IsRunning = false;
        }
    }

    private static string PriorityLabel(OnlineSourcePriority priority) =>
        priority switch
        {
            OnlineSourcePriority.QQMusicFirst => "QQ 音乐优先",
            OnlineSourcePriority.NeteaseOnly => "仅网易云音乐",
            OnlineSourcePriority.QQMusicOnly => "仅 QQ 音乐",
            _ => "网易云音乐优先",
        };

    [RelayCommand]
    private void Cancel() => _cts?.Cancel();

    public void Dispose()
    {
        _cts?.Cancel();
        _cts?.Dispose();
        _cts = null;
    }
}
