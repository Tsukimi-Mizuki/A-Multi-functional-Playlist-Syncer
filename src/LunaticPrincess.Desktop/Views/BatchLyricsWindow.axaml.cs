using Avalonia.Controls;
using Avalonia.Interactivity;
using LunaticPrincess.Desktop.Services;

namespace LunaticPrincess.Desktop.Views;

public partial class BatchLyricsWindow : Window
{
    public BatchLyricsWindow()
    {
        InitializeComponent();
        UiAssets.ApplyWindowIcon(this);
        VisualThemeManager.ApplyDialog(this);
    }

    private void OnCloseClick(object? sender, RoutedEventArgs e) => Close();

    protected override void OnClosed(EventArgs e)
    {
        (DataContext as IDisposable)?.Dispose();
        base.OnClosed(e);
    }
}
