using Avalonia.Controls;
using Avalonia.Interactivity;
using NeteaseMusicManager.Desktop.Services;

namespace NeteaseMusicManager.Desktop.Views;

public partial class LyricsEditorWindow : Window
{
    public LyricsEditorWindow()
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
