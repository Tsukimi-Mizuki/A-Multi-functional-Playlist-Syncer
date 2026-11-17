import 'package:fluent_ui/fluent_ui.dart';
import 'package:lunatic_princess/controller.dart';
import 'package:lunatic_princess/src/rust/api/app.dart';
import 'package:lunatic_princess/widgets/info_row.dart';
import 'package:lunatic_princess/widgets/login_dialog.dart';

class OverviewPage extends StatelessWidget {
  const OverviewPage({super.key, required this.controller});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final c = controller;
    final app = c.appState!;
    final v = c.validation;

    final warnings = <(String, String, InfoBarSeverity)>[
      if (app.sessionError != null)
        ('登录文件读取失败', app.sessionError!, InfoBarSeverity.warning),
      if (app.settingsWarning != null)
        ('设置文件读取失败', app.settingsWarning!, InfoBarSeverity.warning),
      if (v != null && v.status == 'unverified')
        ('暂时无法验证登录状态', v.message ?? '已保留登录，稍后会再次验证。', InfoBarSeverity.warning),
      if (v != null && v.status == 'loggedOut')
        ('登录已失效', v.message ?? '请重新扫码登录。', InfoBarSeverity.error),
    ];

    return ScaffoldPage.scrollable(
      header: const PageHeader(title: Text('概览')),
      children: [
        for (final w in warnings)
          Padding(
            padding: const EdgeInsets.only(bottom: 12),
            child: InfoBar(
              title: Text(w.$1),
              content: Text(w.$2),
              severity: w.$3,
              isLong: true,
            ),
          ),
        _AccountCard(controller: c),
        const SizedBox(height: 12),
        _EnvironmentCard(app: app),
      ],
    );
  }
}

String _initials(String? nickname) {
  final name = (nickname ?? '').trim();
  if (name.isEmpty) return '?';
  // 取第一个字符就够了：中文昵称一个字，英文昵称一个字首字母。用 runes 而不是
  // substring，避免表情昵称把代理对拆成两个半个字符。
  return String.fromCharCode(name.runes.first).toUpperCase();
}

class _AccountCard extends StatelessWidget {
  const _AccountCard({required this.controller});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final c = controller;
    final account = c.account;
    final theme = FluentTheme.of(context);
    final res = theme.resources;
    final loggedIn = account.loggedIn;
    final vip = account.vipType > 0;
    final status = loggedIn
        ? (vip ? '已登录 · 会员' : '已登录')
        : (c.starting ? '正在验证登录状态…' : '未登录，部分功能不可用');

    return Card(
      padding: const EdgeInsets.all(20),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: [
          Container(
            width: 56,
            height: 56,
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              color: loggedIn
                  ? theme.accentColor.defaultBrushFor(theme.brightness)
                  : res.subtleFillColorSecondary,
            ),
            alignment: Alignment.center,
            child: loggedIn
                ? Text(
                    _initials(account.nickname),
                    style: TextStyle(
                      fontSize: 22,
                      fontWeight: FontWeight.w600,
                      color: theme.brightness == Brightness.dark
                          ? Colors.black
                          : Colors.white,
                    ),
                  )
                : WindowsIcon(
                    WindowsIcons.contact,
                    size: 24,
                    color: res.textFillColorSecondary,
                  ),
          ),
          const SizedBox(width: 16),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(
                  loggedIn ? (account.nickname ?? '已登录') : '未登录',
                  style: theme.typography.subtitle,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
                const SizedBox(height: 2),
                Text(
                  status,
                  style: TextStyle(color: res.textFillColorSecondary),
                ),
              ],
            ),
          ),
          if (c.starting)
            const SizedBox(width: 20, height: 20, child: ProgressRing(strokeWidth: 2.5))
          else if (!loggedIn)
            FilledButton(
              onPressed: () => showLoginDialog(context),
              child: const Text('扫码登录'),
            ),
        ],
      ),
    );
  }
}

class _EnvironmentCard extends StatelessWidget {
  const _EnvironmentCard({required this.app});

  final AppStateDto app;

  @override
  Widget build(BuildContext context) {
    final theme = FluentTheme.of(context);
    return Card(
      padding: const EdgeInsets.fromLTRB(20, 16, 20, 16),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('运行环境', style: theme.typography.bodyStrong),
          const SizedBox(height: 8),
          InfoRow(label: '版本', value: app.version, monospace: true),
          InfoRow(
            label: '运行模式',
            value: app.isPortable ? '便携模式（数据在程序目录）' : '标准模式（数据在用户目录）',
          ),
          InfoRow(label: '数据目录', value: app.dataDir, monospace: true, copyable: true),
          InfoRow(
            label: '下载目录',
            value: app.downloadDirectory,
            monospace: true,
            copyable: true,
          ),
        ],
      ),
    );
  }
}
