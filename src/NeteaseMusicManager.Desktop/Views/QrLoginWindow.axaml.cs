using Avalonia.Controls;
using Avalonia.Interactivity;
using Avalonia.Threading;
using NeteaseMusicManager.Core.Models;
using NeteaseMusicManager.Desktop.Services;
using NeteaseMusicManager.Desktop.ViewModels;

namespace NeteaseMusicManager.Desktop.Views;

public partial class QrLoginWindow : Window
{
    private QrLoginViewModel? _viewModel;

    public QrLoginWindow()
    {
        InitializeComponent();
        UiAssets.ApplyWindowIcon(this);
        VisualThemeManager.ApplyDialog(this);
    }

    protected override void OnOpened(EventArgs e)
    {
        base.OnOpened(e);
        if (DataContext is not QrLoginViewModel viewModel)
        {
            return;
        }

        _viewModel = viewModel;
        viewModel.Completed += OnLoginCompleted;
        _ = viewModel.StartAsync();
    }

    protected override void OnClosed(EventArgs e)
    {
        if (_viewModel is not null)
        {
            _viewModel.Completed -= OnLoginCompleted;
            _viewModel.Cancel();
            _viewModel = null;
        }

        base.OnClosed(e);
    }

    private void OnLoginCompleted(NeteaseSession? session) =>
        Dispatcher.UIThread.Post(() => Close(session));

    private void OnCancelClick(object? sender, RoutedEventArgs e) => Close(null);
}