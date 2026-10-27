using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Platform.Storage;
using LunaticPrincess.Desktop.Services;
using LunaticPrincess.Desktop.ViewModels;

namespace LunaticPrincess.Desktop.Views;

public partial class NcmImportWindow : Window
{
    private NcmImportViewModel? _viewModel;

    public NcmImportWindow()
    {
        InitializeComponent();
        UiAssets.ApplyWindowIcon(this);
        VisualThemeManager.ApplyDialog(this);
    }

    protected override void OnOpened(EventArgs e)
    {
        base.OnOpened(e);
        _viewModel = DataContext as NcmImportViewModel;
    }

    protected override void OnClosed(EventArgs e)
    {
        _viewModel?.CancelCommand.Execute(null);
        _viewModel = null;
        base.OnClosed(e);
    }

    private async void OnAddFoldersClick(object? sender, RoutedEventArgs e)
    {
        if (_viewModel is not { } viewModel)
        {
            return;
        }

        var folders = await PickFoldersAsync("选择包含 .ncm 文件的文件夹", allowMultiple: true);
        if (folders.Count > 0)
        {
            await viewModel.AddFoldersAsync(folders);
        }
    }

    private async void OnBrowseOutputClick(object? sender, RoutedEventArgs e)
    {
        if (_viewModel is not { } viewModel)
        {
            return;
        }

        var folders = await PickFoldersAsync("选择解密输出目录", allowMultiple: false);
        if (folders.Count > 0)
        {
            viewModel.OutputDirectory = folders[0];
        }
    }

    private void OnCloseClick(object? sender, RoutedEventArgs e)
    {
        _viewModel?.CancelCommand.Execute(null);
        Close();
    }

    private async Task<IReadOnlyList<string>> PickFoldersAsync(string title, bool allowMultiple)
    {
        try
        {
            var folders = await StorageProvider.OpenFolderPickerAsync(new FolderPickerOpenOptions
            {
                Title = title,
                AllowMultiple = allowMultiple,
            });
            return folders
                .Select(folder => folder.TryGetLocalPath())
                .Where(path => !string.IsNullOrWhiteSpace(path))
                .Select(path => path!)
                .ToList();
        }
        catch (Exception ex)
        {
            if (_viewModel is { } viewModel)
            {
                viewModel.StatusText = "选择文件夹失败：" + Services.ErrorFormatter.ToFriendlyMessage(ex);
            }

            return [];
        }
    }
}