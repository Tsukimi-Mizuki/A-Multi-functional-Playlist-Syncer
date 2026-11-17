import 'package:fluent_ui/fluent_ui.dart';
import 'package:lunatic_princess/controller.dart';

class SettingsPage extends StatelessWidget {
  const SettingsPage({super.key, required this.controller});

  final AppController controller;

  static const _themeLabels = {
    ThemeMode.system: '跟随系统',
    ThemeMode.light: '浅色',
    ThemeMode.dark: '深色',
  };

  @override
  Widget build(BuildContext context) {
    final c = controller;
    return ScaffoldPage.scrollable(
      header: const PageHeader(title: Text('设置')),
      children: [
        const InfoBar(
          title: Text('原型提示'),
          content: Text('外观设置会立即生效，但原型版本暂不保存，重启后恢复默认。'),
          isLong: true,
        ),
        const SizedBox(height: 16),
        Card(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              InfoLabel(
                label: '窗口材质',
                child: ComboBox<WindowMaterial>(
                  value: c.material,
                  items: [
                    for (final m in WindowMaterial.values)
                      ComboBoxItem(value: m, child: Text(m.label)),
                  ],
                  onChanged: (m) {
                    if (m != null) c.setMaterial(m);
                  },
                ),
              ),
              const SizedBox(height: 16),
              InfoLabel(
                label: '主题',
                child: ComboBox<ThemeMode>(
                  value: c.themeMode,
                  items: [
                    for (final e in _themeLabels.entries)
                      ComboBoxItem(value: e.key, child: Text(e.value)),
                  ],
                  onChanged: (m) {
                    if (m != null) c.setThemeMode(m);
                  },
                ),
              ),
            ],
          ),
        ),
      ],
    );
  }
}
