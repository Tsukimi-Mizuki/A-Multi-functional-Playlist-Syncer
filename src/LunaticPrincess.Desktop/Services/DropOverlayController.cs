using Avalonia.Controls;
using Avalonia.Input;
using Avalonia.Threading;

namespace LunaticPrincess.Desktop.Services;

/// <summary>
/// 拖放提示层的显隐控制。
/// Avalonia 在拖放目标身份变化（指针跨过相邻控件）时会成对重发 DragLeave/DragEnter，
/// 收到 DragLeave 就立刻隐藏会让提示层闪烁、甚至在指针仍停留在窗口上时保持隐藏，
/// 因此隐藏延迟一小段时间执行，期间收到 DragEnter/DragOver 就取消。
/// </summary>
public sealed class DropOverlayController
{
    private static readonly TimeSpan HideDelay = TimeSpan.FromMilliseconds(180);

    private readonly Control _overlay;
    private readonly DispatcherTimer _hideTimer;

    public DropOverlayController(Control overlay)
    {
        _overlay = overlay;
        _hideTimer = new DispatcherTimer { Interval = HideDelay };
        _hideTimer.Tick += (_, _) => HideNow();
    }

    /// <summary>按拖入数据是否包含文件更新光标效果与提示层。</summary>
    public void Update(DragEventArgs e)
    {
        var accepted = e.DataTransfer.Contains(DataFormat.File);
        e.DragEffects = accepted ? DragDropEffects.Copy : DragDropEffects.None;
        if (accepted)
        {
            _hideTimer.Stop();
            _overlay.IsVisible = true;
        }
        else
        {
            HideNow();
        }
    }

    /// <summary>拖离窗口时延迟隐藏，避免目标切换导致的闪烁。</summary>
    public void HideSoon() => _hideTimer.Start();

    /// <summary>立即隐藏提示层（投放结束或窗口关闭时调用）。</summary>
    public void HideNow()
    {
        _hideTimer.Stop();
        _overlay.IsVisible = false;
    }
}
