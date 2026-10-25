using System.Collections.Specialized;
using Avalonia.Controls;
using Avalonia.Threading;
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
        Closed += OnWindowClosed;
    }

    private void OnWindowClosed(object? sender, EventArgs e)
    {
        if (_viewModel is null)
        {
            return;
        }

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