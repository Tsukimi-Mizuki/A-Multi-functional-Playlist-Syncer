import 'package:fluent_ui/fluent_ui.dart';
import 'package:window_manager/window_manager.dart';

/// 自绘的最小化 / 最大化 / 关闭按钮。
///
/// 不直接用 `WindowCaption`：它内部是 `Expanded`，放进 `TitleBar` 的
/// 右侧区域会把整块拖动区域吃掉；这里只用它导出的按钮部件。
class WindowButtons extends StatefulWidget {
  const WindowButtons({super.key});

  @override
  State<WindowButtons> createState() => _WindowButtonsState();
}

class _WindowButtonsState extends State<WindowButtons> {
  Window? get _window => WindowManager.instance.getCurrent();
  ListenerId? _listener;
  bool _maximized = false;

  @override
  void initState() {
    super.initState();
    _maximized = _window?.isMaximized ?? false;
    _listener = _window?.addListener((event) {
      if (event is WindowMaximizedEvent || event is WindowRestoredEvent) {
        final m = _window?.isMaximized ?? false;
        if (m != _maximized && mounted) setState(() => _maximized = m);
      }
    });
  }

  @override
  void dispose() {
    final id = _listener;
    if (id != null) _window?.removeListener(id);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final brightness = FluentTheme.of(context).brightness;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        WindowCaptionButton.minimize(
          brightness: brightness,
          onPressed: () => _window?.minimize(),
        ),
        MaximizeButtonArea(
          child: _maximized
              ? WindowCaptionButton.unmaximize(
                  brightness: brightness,
                  onPressed: () => _window?.unmaximize(),
                )
              : WindowCaptionButton.maximize(
                  brightness: brightness,
                  onPressed: () => _window?.maximize(),
                ),
        ),
        WindowCaptionButton.close(
          brightness: brightness,
          // 走 close()，会先询问关闭监听器（main.dart 里在那里收尾）。
          onPressed: () => _window?.close(),
        ),
      ],
    );
  }
}
