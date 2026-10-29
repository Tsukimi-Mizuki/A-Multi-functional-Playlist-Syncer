using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Platform.Storage;
using LunaticPrincess.Core.Utilities;
using LunaticPrincess.Desktop.Services;
using LunaticPrincess.Desktop.ViewModels;

namespace LunaticPrincess.Desktop.Views;

public partial class LyricsEditorWindow : Window
{
    public LyricsEditorWindow()
    {
        InitializeComponent();
        UiAssets.ApplyWindowIcon(this);
        VisualThemeManager.ApplyDialog(this);
        // 拖放载入：整个窗口都是投放目标。
        DragDrop.SetAllowDrop(this, true);
        DragDrop.AddDragOverHandler(this, OnFileDragOver);
        DragDrop.AddDragLeaveHandler(this, OnFileDragLeave);
        DragDrop.AddDropHandler(this, OnFileDrop);
    }

    private void OnCloseClick(object? sender, RoutedEventArgs e) => Close();

    /// <summary>拖入内容里带文件（或文件夹）时显示提示层并接受投放。</summary>
    private void OnFileDragOver(object? sender, DragEventArgs e)
    {
        var accepted = e.DataTransfer.Contains(DataFormat.File);
        e.DragEffects = accepted ? DragDropEffects.Copy : DragDropEffects.None;
        DropOverlay.IsVisible = accepted;
        e.Handled = true;
    }

    private void OnFileDragLeave(object? sender, DragEventArgs e)
    {
        DropOverlay.IsVisible = false;
        e.Handled = true;
    }

    private async void OnFileDrop(object? sender, DragEventArgs e)
    {
        DropOverlay.IsVisible = false;
        e.Handled = true;
        if (DataContext is not LyricsEditorViewModel viewModel)
        {
            return;
        }

        try
        {
            var paths = GetDroppedPaths(e);
            if (paths.Count == 0)
            {
                viewModel.StatusText = AudioFileTypes.EmptyTargetMessage;
                return;
            }

            await viewModel.LoadDroppedPathsAsync(paths);
        }
        catch (Exception ex)
        {
            viewModel.StatusText = "载入拖入的文件失败：" + ErrorFormatter.ToFriendlyMessage(ex);
        }
    }

    private static IReadOnlyList<string?> GetDroppedPaths(DragEventArgs e)
    {
        var items = e.DataTransfer.TryGetFiles();
        if (items is null)
        {
            return [];
        }

        return items.Select(item => item.TryGetLocalPath()).ToList();
    }

    private async void OnBatchLyricsClick(object? sender, RoutedEventArgs e)
    {
        if (DataContext is not LyricsEditorViewModel viewModel)
        {
            return;
        }

        try
        {
            var batchViewModel = viewModel.CreateBatchLyricsViewModel();
            await new BatchLyricsWindow { DataContext = batchViewModel }.ShowDialog(this);
        }
        catch (Exception ex)
        {
            await new MessageDialog(
                    "批量歌词",
                    "打开批量歌词窗口失败：" + ErrorFormatter.ToFriendlyMessage(ex),
                    "知道了")
                .ShowDialog(this);
        }
    }

    protected override void OnClosed(EventArgs e)
    {
        (DataContext as IDisposable)?.Dispose();
        base.OnClosed(e);
    }
}
