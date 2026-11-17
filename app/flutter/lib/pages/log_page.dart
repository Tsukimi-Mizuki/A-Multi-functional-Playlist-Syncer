import 'package:fluent_ui/fluent_ui.dart';
import 'package:lunatic_princess/controller.dart';
import 'package:lunatic_princess/format.dart';
import 'package:lunatic_princess/log_merge.dart';

class LogPage extends StatefulWidget {
  const LogPage({super.key, required this.controller});

  final AppController controller;

  @override
  State<LogPage> createState() => _LogPageState();
}

class _LogPageState extends State<LogPage> {
  final _scroll = ScrollController();
  int _lastCount = 0;

  @override
  void initState() {
    super.initState();
    // 进入页面时直接停在最底部（最新日志）。
    WidgetsBinding.instance.addPostFrameCallback((_) => _jumpToEnd());
  }

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  void _jumpToEnd() {
    if (_scroll.hasClients) {
      _scroll.jumpTo(_scroll.position.maxScrollExtent);
    }
  }

  bool get _atBottom =>
      !_scroll.hasClients ||
      _scroll.position.maxScrollExtent - _scroll.offset < 24;

  @override
  Widget build(BuildContext context) {
    final logs = widget.controller.logs;
    // 只有新增日志之前就在底部才自动滚动，避免打断用户翻看历史。
    if (logs.length != _lastCount) {
      final follow = _atBottom;
      _lastCount = logs.length;
      if (follow) {
        WidgetsBinding.instance.addPostFrameCallback((_) => _jumpToEnd());
      }
    }

    return ScaffoldPage(
      header: const PageHeader(title: Text('运行日志')),
      // 用页面默认的内边距，标题与其他页面对齐。
      content: logs.isEmpty
          ? const Center(child: Text('暂无日志'))
          : ListView.builder(
              controller: _scroll,
              padding: const EdgeInsets.symmetric(vertical: 8),
              itemCount: logs.length,
              itemBuilder: (context, i) => _LogRow(line: logs[i]),
            ),
    );
  }
}

class _LogRow extends StatelessWidget {
  const _LogRow({required this.line});

  final LogLine line;

  @override
  Widget build(BuildContext context) {
    final res = FluentTheme.of(context).resources;
    final (icon, color) = switch (line.level) {
      'success' => (WindowsIcons.accept, Colors.green),
      'warning' => (WindowsIcons.warning, Colors.orange),
      'error' => (WindowsIcons.error_badge, Colors.red),
      _ => (WindowsIcons.info, res.textFillColorSecondary),
    };
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            formatClock(line.timestampMs),
            style: TextStyle(
              color: res.textFillColorSecondary,
              fontFeatures: const [FontFeature.tabularFigures()],
            ),
          ),
          const SizedBox(width: 12),
          Padding(
            padding: const EdgeInsets.only(top: 3),
            child: WindowsIcon(icon, size: 13, color: color),
          ),
          const SizedBox(width: 8),
          Expanded(
            child: SelectableText(
              line.message,
              style: line.level == 'info' ? null : TextStyle(color: color),
            ),
          ),
        ],
      ),
    );
  }
}
