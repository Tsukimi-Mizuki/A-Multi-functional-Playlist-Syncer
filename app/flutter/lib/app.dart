import 'dart:async';

import 'package:desktop_drop/desktop_drop.dart';
import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter_acrylic/flutter_acrylic.dart' as acrylic;
import 'package:lunatic_princess/controller.dart';
import 'package:lunatic_princess/pages/log_page.dart';
import 'package:lunatic_princess/pages/overview_page.dart';
import 'package:lunatic_princess/pages/playlist_page.dart';
import 'package:lunatic_princess/pages/settings_page.dart';
import 'package:lunatic_princess/src/rust/api/editor.dart';
import 'package:lunatic_princess/widgets/batch_tags_dialog.dart';
import 'package:lunatic_princess/widgets/login_dialog.dart';
import 'package:lunatic_princess/widgets/ncm_import_dialog.dart';
import 'package:lunatic_princess/widgets/sidebar.dart';
import 'package:lunatic_princess/widgets/tag_editor_dialog.dart';
import 'package:lunatic_princess/widgets/window_buttons.dart';
import 'package:lunatic_princess/widgets/word_lyrics_dialog.dart';
import 'package:window_manager/window_manager.dart';

const _fontFamily = 'Microsoft YaHei UI';

class LunaticPrincessApp extends StatefulWidget {
  const LunaticPrincessApp({super.key});

  @override
  State<LunaticPrincessApp> createState() => _LunaticPrincessAppState();
}

class _LunaticPrincessAppState extends State<LunaticPrincessApp> {
  final controller = AppController();

  @override
  void initState() {
    super.initState();
    unawaited(controller.start());
  }

  @override
  void dispose() {
    controller.dispose();
    super.dispose();
  }

  FluentThemeData _theme(Brightness b) {
    // 有窗口材质时让各层背景透明，系统材质才透得出来。
    final transparent = controller.material != WindowMaterial.none;
    return FluentThemeData(brightness: b, fontFamily: _fontFamily).copyWith(
      // Win11 的强调色默认是系统蓝，先固定用它；以后接系统强调色只改这里。
      accentColor: Colors.blue,
      scaffoldBackgroundColor: transparent ? Colors.transparent : null,
    );
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        return FluentApp(
          title: 'Lunatic Princess',
          debugShowCheckedModeBanner: false,
          locale: const Locale('zh', 'CN'),
          themeMode: controller.themeMode,
          theme: _theme(Brightness.light),
          darkTheme: _theme(Brightness.dark),
          builder: (context, child) => _EffectSync(
            material: controller.material,
            child: child ?? const SizedBox.shrink(),
          ),
          home: controller.startupError != null
              ? _StartupErrorPage(message: controller.startupError!)
              : controller.appState == null
              ? const Center(child: ProgressRing())
              : AppShell(controller: controller),
        );
      },
    );
  }
}

/// 亮暗或材质变化时重新设置窗口效果。
class _EffectSync extends StatefulWidget {
  const _EffectSync({required this.material, required this.child});

  final WindowMaterial material;
  final Widget child;

  @override
  State<_EffectSync> createState() => _EffectSyncState();
}

class _EffectSyncState extends State<_EffectSync> {
  WindowMaterial? _appliedMaterial;
  Brightness? _appliedBrightness;

  void _apply(Brightness b) {
    if (_appliedMaterial == widget.material && _appliedBrightness == b) return;
    _appliedMaterial = widget.material;
    _appliedBrightness = b;
    final dark = b == Brightness.dark;
    final effect = switch (widget.material) {
      WindowMaterial.mica => acrylic.WindowEffect.mica,
      WindowMaterial.acrylic => acrylic.WindowEffect.acrylic,
      WindowMaterial.none => acrylic.WindowEffect.disabled,
    };
    // 亚克力需要半透明底色，否则几乎看不出深浅主题。
    final tint = dark ? const Color(0xCC202020) : const Color(0xCCF3F3F3);
    unawaited(
      acrylic.Window.setEffect(
        effect: effect,
        color: tint,
        dark: dark,
      ).catchError((Object _) {}),
    );
  }

  @override
  Widget build(BuildContext context) {
    final b = FluentTheme.of(context).brightness;
    // 在帧后调用平台通道，避免在 build 里产生副作用。
    WidgetsBinding.instance.addPostFrameCallback((_) => _apply(b));
    return widget.child;
  }
}

class _StartupErrorPage extends StatelessWidget {
  const _StartupErrorPage({required this.message});

  final String message;

  @override
  Widget build(BuildContext context) {
    return ScaffoldPage(
      content: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 560),
          child: InfoBar(
            title: const Text('启动失败'),
            content: SelectableText(message),
            severity: InfoBarSeverity.error,
            isLong: true,
          ),
        ),
      ),
    );
  }
}

/// 主外壳：标题栏 + 侧边栏 + 页面，同时接收从资源管理器拖进来的文件。
class AppShell extends StatefulWidget {
  const AppShell({super.key, required this.controller});

  final AppController controller;

  @override
  State<AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<AppShell> {
  bool _dragging = false;
  bool _handlingDrop = false;

  AppController get _controller => widget.controller;

  /// 拖入的文件先交给 lp-core 解析：一首开编辑器，多首（或含多首的文件夹）开批量面板，
  /// 只有 .ncm 时提示去「导入 NCM」。有对话框盖在界面上时不接收，避免塞到看不见的窗口里。
  Future<void> _handleDrop(List<String> paths) async {
    setState(() => _dragging = false);
    if (paths.isEmpty || _handlingDrop) return;
    if (Navigator.of(context).canPop()) return;
    _handlingDrop = true;
    try {
      final result = await resolveDropPaths(paths: paths);
      if (!mounted) return;
      switch (result.kind) {
        case 'audio':
          final path = result.path;
          if (path != null) await showTagEditorDialog(context, path);
        case 'batch':
          final folder = result.files.isEmpty
              ? ''
              : _directoryOf(result.files.first.path);
          await showBatchTagsDialog(context, initialFolder: folder);
        case 'ncm':
          await showNcmImportDialog(context);
        default:
          _controller.showNotice(result.message);
      }
    } catch (error) {
      if (mounted) _controller.showNotice('载入拖入的文件失败：$error');
    } finally {
      _handlingDrop = false;
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = _controller;
    // 侧边栏宽 320 逻辑像素，窗口再窄就只留图标（Win11 的紧凑态）。
    return DropTarget(
      onDragEntered: (_) => setState(() => _dragging = true),
      onDragExited: (_) => setState(() => _dragging = false),
      onDragDone: (details) => unawaited(
        _handleDrop(details.files.map((file) => file.path).toList()),
      ),
      child: LayoutBuilder(
        builder: (context, constraints) {
          final compact = constraints.maxWidth < 1008;
          return Stack(
            children: [
              Column(
                children: [
                  _AppTitleBar(controller: c),
                  Expanded(
                    child: Row(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: [
                        AppSidebar(controller: c, compact: compact),
                        Expanded(child: _buildPage(c)),
                      ],
                    ),
                  ),
                ],
              ),
              if (_dragging) const Positioned.fill(child: _DropOverlay()),
            ],
          );
        },
      ),
    );
  }

  Widget _buildPage(AppController c) {
    switch (c.selectedKey) {
      case kKeyLog:
        return LogPage(controller: c);
      case kKeySettings:
        return SettingsPage(controller: c);
      case kKeyOverview:
        return OverviewPage(controller: c);
      default:
        return PlaylistPage(controller: c);
    }
  }
}

/// 拖放提示层：鼠标拖到窗口上时盖住界面，说明松手会发生什么。
class _DropOverlay extends StatelessWidget {
  const _DropOverlay();

  @override
  Widget build(BuildContext context) {
    final theme = FluentTheme.of(context);
    return IgnorePointer(
      child: Container(
        color: theme.accentColor.withValues(alpha: 0.10),
        alignment: Alignment.center,
        child: DecoratedBox(
          decoration: BoxDecoration(
            color: theme.resources.solidBackgroundFillColorBase,
            border: Border.all(color: theme.accentColor, width: 2),
            borderRadius: BorderRadius.circular(12),
          ),
          child: const Padding(
            padding: EdgeInsets.symmetric(horizontal: 28, vertical: 18),
            child: Text(
              '松开以打开：一首歌用编辑器打开，多首交给批量修改标签',
              style: TextStyle(fontSize: 14),
            ),
          ),
        ),
      ),
    );
  }
}

/// 取文件所在目录（拖入的文件夹展开后的第一个文件的上级目录）。
String _directoryOf(String path) {
  final index = path.lastIndexOf(RegExp(r'[\\/]'));
  if (index <= 0) return path;
  return path.substring(0, index);
}

class _AppTitleBar extends StatelessWidget {
  const _AppTitleBar({required this.controller});

  static const height = 40.0;

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final window = WindowManager.instance.getCurrent();
    final theme = FluentTheme.of(context);
    return DecoratedBox(
      decoration: BoxDecoration(
        border: Border(
          bottom: BorderSide(
            width: 0.5,
            color: theme.resources.dividerStrokeColorDefault,
          ),
        ),
      ),
      child: SizedBox(
        height: height,
        child: Row(
          children: [
            Expanded(
              child: GestureDetector(
                behavior: HitTestBehavior.opaque,
                onPanStart: (_) => window?.startDragging(),
                onDoubleTap: () {
                  if (window == null) return;
                  window.isMaximized ? window.unmaximize() : window.maximize();
                },
                child: Padding(
                  padding: const EdgeInsetsDirectional.only(start: 16, end: 16),
                  child: Align(
                    alignment: AlignmentDirectional.centerStart,
                    child: Text(
                      'Lunatic Princess',
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: theme.typography.body?.copyWith(
                        color: theme.resources.textFillColorSecondary,
                      ),
                    ),
                  ),
                ),
              ),
            ),
            Button(
              onPressed: () => unawaited(showNcmImportDialog(context)),
              child: const Row(
                mainAxisSize: MainAxisSize.min,
                children: <Widget>[
                  WindowsIcon(WindowsIcons.download, size: 14),
                  SizedBox(width: 6),
                  Text('导入 NCM'),
                ],
              ),
            ),
            const SizedBox(width: 8),
            Button(
              onPressed: () => unawaited(
                showWordLyricsDialog(
                  context,
                  initialFolder:
                      controller.settings?.settings.libraryDirectory ?? '',
                ),
              ),
              child: const Row(
                mainAxisSize: MainAxisSize.min,
                children: <Widget>[
                  WindowsIcon(WindowsIcons.font, size: 14),
                  SizedBox(width: 6),
                  Text('歌词转逐字'),
                ],
              ),
            ),
            const SizedBox(width: 8),
            _AccountButton(controller: controller),
            const SizedBox(width: 8),
            const Align(
              alignment: Alignment.topRight,
              widthFactor: 1,
              child: WindowButtons(),
            ),
          ],
        ),
      ),
    );
  }
}

class _AccountButton extends StatefulWidget {
  const _AccountButton({required this.controller});

  final AppController controller;

  @override
  State<_AccountButton> createState() => _AccountButtonState();
}

class _AccountButtonState extends State<_AccountButton> {
  final _flyout = FlyoutController();

  @override
  void dispose() {
    _flyout.dispose();
    super.dispose();
  }

  Future<void> _confirmLogout() async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => ContentDialog(
        title: const Text('退出登录'),
        content: const Text('确定要退出当前账号吗？'),
        actions: [
          Button(
            child: const Text('取消'),
            onPressed: () => Navigator.pop(ctx, false),
          ),
          FilledButton(
            child: const Text('退出登录'),
            onPressed: () => Navigator.pop(ctx, true),
          ),
        ],
      ),
    );
    if (ok == true) await widget.controller.doLogout();
  }

  @override
  Widget build(BuildContext context) {
    final account = widget.controller.account;
    if (!account.loggedIn) {
      return Padding(
        padding: const EdgeInsets.symmetric(vertical: 4),
        child: Button(
          onPressed: () => showLoginDialog(context),
          child: const Text('扫码登录'),
        ),
      );
    }
    return FlyoutTarget(
      controller: _flyout,
      child: HyperlinkButton(
        onPressed: () {
          _flyout.showFlyout<void>(
            builder: (ctx) => MenuFlyout(
              items: [
                MenuFlyoutItem(
                  leading: const WindowsIcon(WindowsIcons.sign_out),
                  text: const Text('退出登录'),
                  onPressed: () {
                    Flyout.of(ctx).close();
                    unawaited(_confirmLogout());
                  },
                ),
              ],
            ),
          );
        },
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            const WindowsIcon(WindowsIcons.contact),
            const SizedBox(width: 8),
            Text(account.nickname ?? '已登录'),
          ],
        ),
      ),
    );
  }
}