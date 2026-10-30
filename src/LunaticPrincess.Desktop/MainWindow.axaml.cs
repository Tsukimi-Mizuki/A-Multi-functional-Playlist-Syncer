using System.Collections.Specialized;
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Interactivity;
using Avalonia.Media;
using Avalonia.Platform.Storage;
using Avalonia.Threading;
using Avalonia.VisualTree;
using LunaticPrincess.Desktop.Services;
using LunaticPrincess.Desktop.ViewModels;

namespace LunaticPrincess.Desktop;

public partial class MainWindow : Window
{
    // 自绘标题栏里最大化按钮的两种图标：普通窗口显示“最大化”，最大化后显示“还原”。
    private static readonly Geometry MaximizeIconGeometry = Geometry.Parse("M0.5,0.5 H9.5 V9.5 H0.5 Z");
    private static readonly Geometry RestoreIconGeometry = Geometry.Parse("M0.5,2.5 H7.5 V9.5 H0.5 Z M2.5,0.5 H9.5 V7.5");

    /// <summary>标题栏按下后移动超过这个距离（DIP）才算拖动。</summary>
    private const double TitleBarDragThreshold = 4;

    private readonly DispatcherTimer _longPressTimer;
    private readonly DropOverlayController _dropOverlay;
    private MainWindowViewModel? _viewModel;
    private PointerPressedEventArgs? _titleBarPressArgs;
    private Point _titleBarPressPoint;

    public MainWindow()
    {
        InitializeComponent();
        UiAssets.ApplyWindowIcon(this);
        VisualThemeManager.ApplyMainWindow(this);
        PropertyChanged += OnWindowPropertyChanged;
        UpdateCaptionButtons();

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

        // 拖放载入：整个主窗口都是投放目标，拖入音频文件直接打开歌词编辑器。
        _dropOverlay = new DropOverlayController(DropOverlay);
        DragDrop.SetAllowDrop(this, true);
        DragDrop.AddDragEnterHandler(this, OnFileDragEnter);
        DragDrop.AddDragOverHandler(this, OnFileDragOver);
        DragDrop.AddDragLeaveHandler(this, OnFileDragLeave);
        DragDrop.AddDropHandler(this, OnFileDrop);
    }

    // ===== 拖放载入 =====

    private void OnFileDragEnter(object? sender, DragEventArgs e)
    {
        _dropOverlay.Update(e);
        e.Handled = true;
    }

    private void OnFileDragOver(object? sender, DragEventArgs e)
    {
        _dropOverlay.Update(e);
        e.Handled = true;
    }

    private void OnFileDragLeave(object? sender, DragEventArgs e)
    {
        _dropOverlay.HideSoon();
        e.Handled = true;
    }

    private async void OnFileDrop(object? sender, DragEventArgs e)
    {
        _dropOverlay.HideNow();
        e.Handled = true;
        if (_viewModel is null)
        {
            return;
        }

        var paths = e.DataTransfer.TryGetFiles()?
            .Select(item => item.TryGetLocalPath())
            .ToList() ?? [];
        await _viewModel.HandleDroppedTargetsAsync(paths);
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

    // ===== 自绘标题栏：拖动、双击最大化、窗口按钮 =====

    private void OnTitleBarPointerPressed(object? sender, PointerPressedEventArgs e)
    {
        if (!e.GetCurrentPoint(this).Properties.IsLeftButtonPressed)
        {
            return;
        }

        // 双击标题栏切换最大化；三击及以上忽略，避免和系统的双击节流打架。
        if (e.ClickCount >= 2)
        {
            _titleBarPressArgs = null;
            ToggleWindowState();
            e.Handled = true;
            return;
        }

        // 先只记下按下位置，等指针真正移动了再开始拖动。
        // BeginMoveDrag 会进入系统的移动循环，把随后的第二次按下吞掉，
        // 那样双击标题栏永远不会收到 ClickCount == 2。
        _titleBarPressArgs = e;
        _titleBarPressPoint = e.GetPosition(this);
    }

    private void OnTitleBarPointerMoved(object? sender, PointerEventArgs e)
    {
        if (_titleBarPressArgs is null)
        {
            return;
        }

        if (!e.GetCurrentPoint(this).Properties.IsLeftButtonPressed)
        {
            _titleBarPressArgs = null;
            return;
        }

        var position = e.GetPosition(this);
        if (Math.Abs(position.X - _titleBarPressPoint.X) < TitleBarDragThreshold
            && Math.Abs(position.Y - _titleBarPressPoint.Y) < TitleBarDragThreshold)
        {
            return;
        }

        var pressArgs = _titleBarPressArgs;
        _titleBarPressArgs = null;
        // 交给系统做窗口移动：最大化时 Windows 会自动先还原再跟随拖动，顺带保留贴边贴顶。
        BeginMoveDrag(pressArgs);
    }

    private void OnTitleBarPointerReleased(object? sender, PointerReleasedEventArgs e)
    {
        _titleBarPressArgs = null;
    }

    private void OnTitleBarPointerCaptureLost(object? sender, PointerCaptureLostEventArgs e)
    {
        _titleBarPressArgs = null;
    }

    private void OnMinimizeClick(object? sender, RoutedEventArgs e)
    {
        WindowState = WindowState.Minimized;
    }

    private void OnMaximizeClick(object? sender, RoutedEventArgs e)
    {
        ToggleWindowState();
    }

    private void OnCloseClick(object? sender, RoutedEventArgs e)
    {
        Close();
    }

    private void ToggleWindowState()
    {
        WindowState = WindowState == WindowState.Maximized
            ? WindowState.Normal
            : WindowState.Maximized;
    }

    private void OnWindowPropertyChanged(object? sender, AvaloniaPropertyChangedEventArgs e)
    {
        if (e.Property == WindowStateProperty)
        {
            UpdateCaptionButtons();
        }
    }

    /// <summary>最大化/还原按钮的图标、提示与无障碍名称跟随窗口状态切换。</summary>
    private void UpdateCaptionButtons()
    {
        var maximized = WindowState == WindowState.Maximized;
        MaximizeGlyph.Data = maximized ? RestoreIconGeometry : MaximizeIconGeometry;
        ToolTip.SetTip(MaximizeButton, maximized ? "还原" : "最大化");
        AutomationProperties.SetName(MaximizeButton, maximized ? "还原窗口" : "最大化窗口");
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
        if (row?.DataContext is not SongRowViewModel)
        {
            return;
        }

        _longPressTimer.Stop();
        _longPressTimer.Start();
    }

    private void OnSongGridPointerReleased(object? sender, PointerReleasedEventArgs e)
    {
        _longPressTimer.Stop();
    }

    private void OnSongGridPointerCaptureLost(object? sender, PointerCaptureLostEventArgs e)
    {
        _longPressTimer.Stop();
    }

    private void OnLongPressTimerTick(object? sender, EventArgs e)
    {
        _longPressTimer.Stop();
        _viewModel?.ToggleSelectionCheckboxes();
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
