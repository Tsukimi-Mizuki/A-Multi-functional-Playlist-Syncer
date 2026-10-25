using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Platform.Storage;
using NeteaseMusicManager.Desktop.Services;
using NeteaseMusicManager.Desktop.ViewModels;

namespace NeteaseMusicManager.Desktop.Views;

public partial class SettingsWindow : Window
{
    public SettingsWindow()
    {
        InitializeComponent();
        UiAssets.ApplyWindowIcon(this);
        VisualThemeManager.ApplyDialog(this, VisualThemeManager.CurrentStyle);
    }

    private SettingsViewModel? ViewModel => DataContext as SettingsViewModel;

    private async void OnBrowseDownloadClick(object? sender, RoutedEventArgs e)
    {
        var path = await PickFolderAsync("选择下载目录");
        if (path is not null && ViewModel is { } viewModel)
        {
            viewModel.DownloadDirectory = path;
        }
    }

    private async void OnBrowseLibraryClick(object? sender, RoutedEventArgs e)
    {
        var path = await PickFolderAsync("选择本地曲库目录");
        if (path is not null && ViewModel is { } viewModel)
        {
            viewModel.LibraryDirectory = path;
        }
    }

    private async Task<string?> PickFolderAsync(string title)
    {
        try
        {
            var folders = await StorageProvider.OpenFolderPickerAsync(new FolderPickerOpenOptions
            {
                Title = title,
                AllowMultiple = false,
            });
            return folders
                .Select(folder => folder.TryGetLocalPath())
                .FirstOrDefault(path => !string.IsNullOrWhiteSpace(path));
        }
        catch (Exception ex)
        {
            if (ViewModel is { } viewModel)
            {
                viewModel.ValidationMessage = "选择目录失败：" + ErrorFormatter.ToFriendlyMessage(ex);
            }

            return null;
        }
    }

    private async void OnSaveClick(object? sender, RoutedEventArgs e)
    {
        if (ViewModel is not { } viewModel)
        {
            return;
        }

        SaveButton.IsEnabled = false;
        try
        {
            var saved = await viewModel.SaveAsync();
            if (saved is not null)
            {
                Close(saved);
            }
        }
        catch (Exception ex)
        {
            viewModel.ValidationMessage = "保存设置失败：" + ErrorFormatter.ToFriendlyMessage(ex);
        }
        finally
        {
            SaveButton.IsEnabled = true;
        }
    }

    private void OnCancelClick(object? sender, RoutedEventArgs e) => Close(null);
}