using Avalonia.Controls;
using Avalonia.Interactivity;
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
    }

    private void OnCloseClick(object? sender, RoutedEventArgs e) => Close();

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
