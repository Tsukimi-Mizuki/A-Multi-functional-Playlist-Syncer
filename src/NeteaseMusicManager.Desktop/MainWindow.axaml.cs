using System.Collections.Specialized;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Threading;
using Avalonia.VisualTree;
using NeteaseMusicManager.Desktop.Services;
using NeteaseMusicManager.Desktop.ViewModels;

namespace NeteaseMusicManager.Desktop;

public partial class MainWindow : Window
{
    private readonly DispatcherTimer _longPressTimer;
    private MainWindowViewModel? _viewModel;
    private SongRowViewModel? _longPressRow;

    public MainWindow()
    {
        InitializeComponent();
        UiAssets.ApplyWindowIcon(this);
        VisualThemeManager.ApplyMainWindow(this);

        _longPressTimer = new DispatcherTimer
        {
            Interval = TimeSpan.FromMilliseconds(550),
        };
        _longPressTimer.Tick += OnLongPressTimerTick;

        // DataGrid 会处理行点击，使用 handledEventsToo 才能收到长按所需的按下/释放事件。
        SongGrid.AddHandler(
            InputElement.PointerPressedEvent,
            OnSongGridPointerPressed,
            RoutingStrategies.Tunnel,
            handledEventsToo: true);
        SongGrid.AddHandler(
            InputElement.PointerReleasedEvent,
            OnSongGridPointerReleased,
            RoutingStrategies.Tunnel,
            handledEventsToo: true);
        SongGrid.AddHandler(
            InputElement.PointerCaptureLostEvent,
            OnSongGridPointerCaptureLost,
            RoutingStrategies.Tunnel,
            handledEventsToo: true);
    }

    public MainWindow(MainWindowViewModel viewModel)
        : this()
    {
        _viewModel = viewModel;
        DataContext = viewModel;
        viewModel.LogEntries.CollectionChanged += OnLogEntriesChanged;
        Opened += OnWindowOpened;
        Closed += OnWindowClosed;
    }

    private void OnWindowOpened(object? sender, EventArgs e)
    {
        Opened -= OnWindowOpened;
        // 透明/亚克力后端在窗口句柄创建后才会真正生效，打开时再同步一次。
        VisualThemeManager.ApplyMainWindow(this);
    }

    private void OnWindowClosed(object? sender, EventArgs e)
    {
        Opened -= OnWindowOpened;
        Closed -= OnWindowClosed;
        _longPressTimer.Stop();
        _longPressTimer.Tick -= OnLongPressTimerTick;
        if (_viewModel is null)
        {
            return;
        }

        _viewModel.LogEntries.CollectionChanged -= OnLogEntriesChanged;
        _viewModel.Shutdown();
        _viewModel = null;
    }

    private void OnSongGridPointerPressed(object? sender, PointerPressedEventArgs e)
    {
        if (!e.GetCurrentPoint(SongGrid).Properties.IsLeftButtonPressed)
        {
            return;
        }

        if (e.Source is not Visual source ||
            source is CheckBox or Button ||
            source.FindAncestorOfType<CheckBox>() is not null ||
            source.FindAncestorOfType<Button>() is not null)
        {
            return;
        }

        var row = source.FindAncestorOfType<DataGridRow>();
        if (row?.DataContext is not SongRowViewModel songRow)
        {
            return;
        }

        _longPressTimer.Stop();
        _longPressRow = songRow;
        _longPressTimer.Start();
    }

    private void OnSongGridPointerReleased(object? sender, PointerReleasedEventArgs e)
    {
        _longPressTimer.Stop();
        _longPressRow = null;
    }

    private void OnSongGridPointerCaptureLost(object? sender, PointerCaptureLostEventArgs e)
    {
        _longPressTimer.Stop();
        _longPressRow = null;
    }

    private void OnLongPressTimerTick(object? sender, EventArgs e)
    {
        _longPressTimer.Stop();
        if (_longPressRow is { } row)
        {
            row.ShowSelectionBox = true;
            _longPressRow = null;
        }
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
