using System.Collections.Specialized;
using Avalonia.Controls;
using Avalonia.Threading;
using NeteaseMusicManager.Core.Models;
using NeteaseMusicManager.Desktop.Services;
using NeteaseMusicManager.Desktop.ViewModels;

namespace NeteaseMusicManager.Desktop;

public partial class MainWindow : Window
{
    private MainWindowViewModel? _viewModel;

    public MainWindow()
    {
        InitializeComponent();
        UiAssets.ApplyWindowIcon(this);
    }

    public MainWindow(MainWindowViewModel viewModel)
        : this()
    {
        _viewModel = viewModel;
        DataContext = viewModel;
        viewModel.LogEntries.CollectionChanged += OnLogEntriesChanged;
        viewModel.VisualStyleChanged += OnVisualStyleChanged;
        ApplyVisualStyle(viewModel.SelectedVisualStyle.Value);
        Opened += OnWindowOpened;
        Closed += OnWindowClosed;
    }

    private void OnWindowOpened(object? sender, EventArgs e)
    {
        Opened -= OnWindowOpened;
        // 透明/亚克力后端在窗口句柄创建后才会真正生效，打开时再同步一次。
        if (_viewModel is not null)
        {
            ApplyVisualStyle(_viewModel.SelectedVisualStyle.Value);
        }
    }

    private void OnVisualStyleChanged(AppVisualStyle style)
    {
        if (Dispatcher.UIThread.CheckAccess())
        {
            ApplyVisualStyle(style);
        }
        else
        {
            Dispatcher.UIThread.Post(() => ApplyVisualStyle(style));
        }
    }

    private void ApplyVisualStyle(AppVisualStyle style) => VisualThemeManager.Apply(this, style);

    private void OnWindowClosed(object? sender, EventArgs e)
    {
        Opened -= OnWindowOpened;
        Closed -= OnWindowClosed;
        if (_viewModel is null)
        {
            return;
        }

        _viewModel.VisualStyleChanged -= OnVisualStyleChanged;
        _viewModel.LogEntries.CollectionChanged -= OnLogEntriesChanged;
        _viewModel.Shutdown();
        _viewModel = null;
    }

    private void OnLogEntriesChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        if (e.Action != NotifyCollectionChangedAction.Add)
        {
            return;
        }

        Dispatcher.UIThread.Post(() => LogScrollViewer.ScrollToEnd());
    }
}
