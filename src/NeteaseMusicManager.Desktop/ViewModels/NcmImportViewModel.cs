using System.Collections.ObjectModel;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using NeteaseMusicManager.Core.Download;
using NeteaseMusicManager.Core.Ncm;
using NeteaseMusicManager.Core.Settings;
using NeteaseMusicManager.Desktop.Services;

namespace NeteaseMusicManager.Desktop.ViewModels;

/// <summary>解密导入结果表中的一行。</summary>
public sealed class NcmImportResultRow
{
    public NcmImportResultRow(NcmDecryptResult result)
    {
        FileName = Path.GetFileName(result.SourcePath);
        SourcePath = result.SourcePath;
        Success = result.Success;
        OutputPath = result.OutputPath;
        ErrorMessage = result.ErrorMessage;
        Format = result.Format;
    }

    public string FileName { get; }
    public string SourcePath { get; }
    public bool Success { get; }
    public string? OutputPath { get; }
    public string? ErrorMessage { get; }
    public string? Format { get; }

    public string StatusText => Success
        ? $"已解密{(string.IsNullOrWhiteSpace(Format) ? string.Empty : $" · {Format.ToUpperInvariant()}")} → {Path.GetFileName(OutputPath ?? string.Empty)}"
        : $"失败：{ErrorMessage ?? "未知错误"}";

    public string StatusKey => Success ? "success" : "error";
    public string ToolTipText => Success ? OutputPath ?? string.Empty : ErrorMessage ?? string.Empty;
}

/// <summary>NCM 批量导入对话框：选择文件夹、递归发现 .ncm、批量解密并展示结果。</summary>
public sealed partial class NcmImportViewModel : ObservableObject, IDisposable
{
    private readonly INcmImportService _importService;
    private readonly List<string> _ncmFiles = [];
    private CancellationTokenSource? _cts;
    private bool _disposed;

    public NcmImportViewModel(INcmImportService importService, AppSettings settings)
    {
        _importService = importService;
        OutputDirectory = settings.DownloadDirectory;
    }

    public ObservableCollection<string> Folders { get; } = [];
    public ObservableCollection<NcmImportResultRow> Results { get; } = [];

    public int ImportedCount { get; private set; }
    public int TotalCount { get; private set; }
    public string? LastOutputDirectory { get; private set; }

    [ObservableProperty]
    private string _outputDirectory = string.Empty;

    [ObservableProperty]
    private bool _overwrite;

    [ObservableProperty]
    private string _statusText = "请选择包含 .ncm 文件的文件夹";

    [ObservableProperty]
    private bool _isBusy;

    [ObservableProperty]
    private bool _isScanning;

    [ObservableProperty]
    private double _progressValue;

    [ObservableProperty]
    private bool _hasResults;

    [ObservableProperty]
    private int _discoveredCount;

    [ObservableProperty]
    private string _summaryText = string.Empty;

    public bool CanStartImport => !IsBusy && !IsScanning && _ncmFiles.Count > 0;

    public bool HasFolders => Folders.Count > 0;

    public async Task AddFoldersAsync(IEnumerable<string> folders)
    {
        var added = false;
        foreach (var folder in folders)
        {
            if (string.IsNullOrWhiteSpace(folder))
            {
                continue;
            }

            if (Folders.Any(f => string.Equals(f, folder, StringComparison.OrdinalIgnoreCase)))
            {
                continue;
            }

            Folders.Add(folder);
            added = true;
        }

        if (added)
        {
            OnPropertyChanged(nameof(HasFolders));
            await DiscoverFilesAsync().ConfigureAwait(true);
        }
    }

    private async Task DiscoverFilesAsync()
    {
        if (Folders.Count == 0)
        {
            return;
        }

        IsScanning = true;
        StatusText = "正在查找 .ncm 文件…";
        try
        {
            var folders = Folders.ToArray();
            var files = await Task.Run(() => FindNcmFiles(folders)).ConfigureAwait(true);
            _ncmFiles.Clear();
            _ncmFiles.AddRange(files);
            DiscoveredCount = _ncmFiles.Count;
            StatusText = _ncmFiles.Count > 0
                ? $"已发现 {_ncmFiles.Count} 个 .ncm 文件，点击「开始解密」"
                : "所选文件夹中没有找到 .ncm 文件";
        }
        catch (Exception ex)
        {
            StatusText = "扫描 .ncm 文件失败：" + ErrorFormatter.ToFriendlyMessage(ex);
        }
        finally
        {
            IsScanning = false;
            NotifyStartCanExecuteChanged();
        }
    }

    /// <summary>递归查找 .ncm 文件；无权限的子目录跳过，不影响整体导入。</summary>
    public static IReadOnlyList<string> FindNcmFiles(IEnumerable<string> folders)
    {
        var files = new List<string>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var folder in folders)
        {
            if (!Directory.Exists(folder))
            {
                continue;
            }

            try
            {
                foreach (var file in Directory.EnumerateFiles(folder, "*.ncm", SearchOption.AllDirectories))
                {
                    if (seen.Add(file))
                    {
                        files.Add(file);
                    }
                }
            }
            catch (UnauthorizedAccessException)
            {
                // 跳过不可访问的子目录。
            }
            catch (IOException)
            {
                // 扫描过程中目录被移动/删除，跳过。
            }
        }

        files.Sort(StringComparer.OrdinalIgnoreCase);
        return files;
    }

    [RelayCommand(CanExecute = nameof(CanStartImport))]
    private async Task StartImportAsync()
    {
        if (_ncmFiles.Count == 0)
        {
            await DiscoverFilesAsync().ConfigureAwait(true);
            if (_ncmFiles.Count == 0)
            {
                return;
            }
        }

        var output = OutputDirectory?.Trim() ?? string.Empty;
        if (output.Length == 0 || !Path.IsPathFullyQualified(output))
        {
            StatusText = "请先在设置中选择有效的输出目录";
            return;
        }

        try
        {
            Directory.CreateDirectory(output);
        }
        catch (Exception ex)
        {
            StatusText = "输出目录不可用：" + ErrorFormatter.ToFriendlyMessage(ex);
            return;
        }

        if (_disposed)
        {
            return;
        }

        CancelCurrent();
        _cts = new CancellationTokenSource();
        var token = _cts.Token;

        IsBusy = true;
        NotifyStartCanExecuteChanged();
        Results.Clear();
        ProgressValue = 0;
        SummaryText = string.Empty;
        StatusText = $"开始解密 {_ncmFiles.Count} 个文件…";

        try
        {
            var files = _ncmFiles.ToList();
            var progress = new Progress<NcmImportProgress>(p =>
            {
                if (p.Total > 0)
                {
                    ProgressValue = Math.Min(100, p.Completed * 100.0 / p.Total);
                }

                StatusText = $"正在解密 {p.Completed}/{p.Total}：{Path.GetFileName(p.CurrentFile)}";
            });

            var results = await _importService
                .ImportAsync(files, output, Overwrite, progress, token)
                .ConfigureAwait(true);

            foreach (var result in results)
            {
                Results.Add(new NcmImportResultRow(result));
            }

            ImportedCount = results.Count(r => r.Success);
            TotalCount = results.Count;
            LastOutputDirectory = output;
            ProgressValue = 100;
            HasResults = Results.Count > 0;

            var failed = TotalCount - ImportedCount;
            SummaryText = $"解密完成：成功 {ImportedCount} 个，失败 {failed} 个（共 {TotalCount} 个）";
            StatusText = SummaryText;
        }
        catch (OperationCanceledException)
        {
            StatusText = "导入已取消";
        }
        catch (Exception ex)
        {
            StatusText = "导入失败：" + ErrorFormatter.ToFriendlyMessage(ex);
        }
        finally
        {
            IsBusy = false;
            NotifyStartCanExecuteChanged();
        }
    }

    [RelayCommand]
    private void Cancel()
    {
        try
        {
            _cts?.Cancel();
        }
        catch (ObjectDisposedException)
        {
            // 已结束。
        }
    }

    private void NotifyStartCanExecuteChanged() => StartImportCommand.NotifyCanExecuteChanged();

    private void CancelCurrent()
    {
        var cts = _cts;
        _cts = null;
        if (cts is null)
        {
            return;
        }

        try
        {
            cts.Cancel();
        }
        catch (ObjectDisposedException)
        {
            // 已取消。
        }

        cts.Dispose();
    }

    public void Dispose()
    {
        if (_disposed)
        {
            return;
        }

        _disposed = true;
        CancelCurrent();
    }
}
