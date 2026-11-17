import 'package:fluent_ui/fluent_ui.dart';
import 'package:lunatic_princess/controller.dart';
import 'package:lunatic_princess/widgets/login_dialog.dart';

class OverviewPage extends StatelessWidget {
  const OverviewPage({super.key, required this.controller});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final c = controller;
    final app = c.appState!;
    final account = c.account;
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
            padding: const EdgeInsets.only(bottom: 8),
            child: InfoBar(
              title: Text(w.$1),
              content: Text(w.$2),
              severity: w.$3,
              isLong: true,
            ),
          ),
        Card(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              _row('版本', app.version),
              _row('数据目录', app.dataDir),
              _row('运行模式', app.isPortable ? '便携模式' : '标准模式'),
              _row('下载目录', app.downloadDirectory),
              _row(
                '账号',
                account.loggedIn
                    ? '${account.nickname ?? '已登录'}'
                          '${account.vipType > 0 ? '（会员）' : ''}'
                    : '未登录',
              ),
              if (c.starting)
                const Padding(
                  padding: EdgeInsets.only(top: 8),
                  child: Row(
                    children: [
                      SizedBox(
                        width: 16,
                        height: 16,
                        child: ProgressRing(strokeWidth: 2.5),
                      ),
                      SizedBox(width: 8),
                      Text('正在验证登录状态…'),
                    ],
                  ),
                ),
            ],
          ),
        ),
        if (!account.loggedIn && !c.starting)
          Padding(
            padding: const EdgeInsets.only(top: 16),
            child: Align(
              alignment: Alignment.centerLeft,
              child: FilledButton(
                onPressed: () => showLoginDialog(context),
                child: const Text('扫码登录'),
              ),
            ),
          ),
      ],
    );
  }

  Widget _row(String label, String value) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 88,
            child: Text(
              label,
              style: const TextStyle(color: Color(0xFF8A8A8A)),
            ),
          ),
          Expanded(child: SelectableText(value)),
        ],
      ),
    );
  }
}
