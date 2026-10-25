using System.Diagnostics;
using Avalonia.Controls;
using Avalonia.Platform.Storage;
using NeteaseMusicManager.Core.Models;
using NeteaseMusicManager.Core.Settings;
using NeteaseMusicManager.Desktop.ViewModels;
using NeteaseMusicManager.Desktop.Views;

namespace NeteaseMusicManager.Desktop.Services;

/// <summary>基于 Avalonia Window / StorageProvider 的 UI 服务实现。</summary>
public sealed class DesktopUiService : IDesktopUiService
{
    /// <summary>主窗口；由 App 在创建主窗口后注入。</summary>
    public Window? Owner { get; set; }

    private Window RequireOwner() =>
        Owner ?? throw new InvalidOperationException("主窗口尚未初始化，无法显示对话框");

    public Task<NeteaseSession?> ShowQrLoginAsync(QrLoginViewModel viewModel) =>
        new QrLoginWindow { DataContext = viewModel }.ShowDialog<NeteaseSession?>(RequireOwner());

    public Task<AppSettings?> ShowSettingsAsync(SettingsViewModel viewModel) =>
        new SettingsWindow { DataContext = viewModel }.ShowDialog<AppSettings?>(RequireOwner());

    public Task ShowNcmImportAsync(NcmImportViewModel viewModel) =>
        new NcmImportWindow { DataContext = viewModel }.ShowDialog(RequireOwner());

    public Task ShowLyricsEditorAsync(LyricsEditorViewModel viewModel) =>
        new LyricsEditorWindow { DataContext = viewModel }.ShowDialog(RequireOwner());

    public Task ShowBatchLyricsAsync(BatchLyricsViewModel viewModel) =>
        new BatchLyricsWindow { DataContext = viewModel }.ShowDialog(RequireOwner());

    public async Task<string?> PickAudioFileAsync(string title)
    {
        var files = await RequireOwner().StorageProvider
            .OpenFilePickerAsync(new FilePickerOpenOptions
            {
                Title = title,
                AllowMultiple = false,
                FileTypeFilter =
                [
                    new FilePickerFileType("音频文件")
                    {
                        Patterns = ["*.mp3", "*.flac", "*.m4a", "*.ogg", "*.wav", "*.aac", "*.wma", "*.opus"],
                    },
                    new FilePickerFileType("所有文件") { Patterns = ["*"] },
                ],
            })
            .ConfigureAwait(true);

        return files.FirstOrDefault()?.TryGetLocalPath();
    }

    public async Task<bool> ConfirmAsync(string title, string message, string okText = "确定", string cancelText = "取消")
    {
        var dialog = new MessageDialog(title, message, okText, cancelText);
        return await dialog.ShowDialog<bool>(RequireOwner()).ConfigureAwait(true);
    }

    public async Task ShowMessageAsync(string title, string message, string okText = "知道了")
    {
        var dialog = new MessageDialog(title, message, okText, null);
        await dialog.ShowDialog<bool>(RequireOwner()).ConfigureAwait(true);
    }

    public async Task<IReadOnlyList<string>?> PickFoldersAsync(string title, bool allowMultiple)
    {
        var owner = RequireOwner();
        var folders = await owner.StorageProvider
            .OpenFolderPickerAsync(new FolderPickerOpenOptions
            {
                Title = title,
                AllowMultiple = allowMultiple,
            })
            .ConfigureAwait(true);

        var paths = folders
            .Select(folder => folder.TryGetLocalPath())
            .Where(path => !string.IsNullOrWhiteSpace(path))
            .Select(path => path!)
            .ToList();

        return paths.Count > 0 ? paths : null;
    }

    public async Task<string?> PickFolderAsync(string title)
    {
        var folders = await PickFoldersAsync(title, allowMultiple: false).ConfigureAwait(true);
        return folders?.FirstOrDefault();
    }

    public void OpenFolder(string path)
    {
        if (string.IsNullOrWhiteSpace(path))
        {
            throw new ArgumentException("目录不能为空", nameof(path));
        }

        Directory.CreateDirectory(path);
        Process.Start(new ProcessStartInfo
        {
            FileName = "explorer.exe",
            Arguments = $"\"{path}\"",
            UseShellExecute = true,
        });
    }
}
