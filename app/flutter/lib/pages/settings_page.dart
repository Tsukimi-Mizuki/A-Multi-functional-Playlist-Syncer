import 'package:fluent_ui/fluent_ui.dart';
import 'package:lunatic_princess/controller.dart';
import 'package:lunatic_princess/file_dialogs.dart';
import 'package:lunatic_princess/src/rust/api/settings.dart';
import 'package:lunatic_princess/widgets/info_row.dart';

/// 设置页：外观、下载、本地曲库与目录。改动即时保存到 `settings.json`。
///
/// 只保留 Flutter 端真的有效果的键：Tauri 版的液态玻璃、渲染帧率、背景雾等外观键在 Flutter 里
/// 没有对应物，所以这里不出现，它们仍原样留在设置文件里。
class SettingsPage extends StatefulWidget {
  const SettingsPage({super.key, required this.controller});

  final AppController controller;

  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage> {
  final _templateController = TextEditingController();
  String? _templateSeededFor;

  /// 阈值拖动中的临时值（松手才保存）。
  double? _threshold;

  static const _themeLabels = {
    ThemeMode.system: '跟随系统',
    ThemeMode.light: '浅色',
    ThemeMode.dark: '深色',
  };

  @override
  void dispose() {
    _templateController.dispose();
    super.dispose();
  }

  /// 用系统目录对话框改下载目录或曲库目录，选好即写入设置文件。
  Future<void> _pickDirectory(
    AppController c, {
    required String key,
    required String current,
  }) async {
    final picked = await pickDirectory(initialDirectory: current);
    if (picked == null || !mounted) return;
    await c.saveSettings({key: picked});
  }

  /// 输入框跟随设置加载一次；用户正在输入时（值不同）不覆盖。
  void _syncTemplate(AppSettingsDto s) {
    final key = s.fileNameTemplate;
    if (_templateSeededFor == key) return;
    if (_templateController.text != s.fileNameTemplate) {
      _templateController.text = s.fileNameTemplate;
    }
    _templateSeededFor = key;
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.controller;
    final res = FluentTheme.of(context).resources;
    final loaded = c.settings;
    return ScaffoldPage.scrollable(
      header: const PageHeader(title: Text('设置')),
      children: [
        if (c.settingsError != null) ...[
          InfoBar(
            title: const Text('设置保存失败'),
            content: Text(c.settingsError!),
            severity: InfoBarSeverity.error,
            isLong: true,
          ),
          const SizedBox(height: 12),
        ],
        if (loaded == null)
          const Center(child: Padding(padding: EdgeInsets.all(48), child: ProgressRing()))
        else
          _buildBody(c, res, loaded),
      ],
    );
  }

  Widget _buildBody(AppController c, ResourceDictionary res, SettingsDto loaded) {
    final s = loaded.settings;
    _syncTemplate(s);
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        _Section(
          title: '外观',
          description: '云母与亚克力由系统合成，会跟着桌面壁纸变化。',
          rows: [
            _SettingTile(
              label: '窗口材质',
              description: '亚克力会实时模糊桌面，云母更省电。',
              control: ComboBox<WindowMaterial>(
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
            _SettingTile(
              label: '主题',
              description: '跟随系统时随 Windows 的亮暗设置切换。',
              control: ComboBox<ThemeMode>(
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
        _Section(
          title: '下载',
          description: '下载目录、音频质量与文件命名。',
          rows: [
            _SettingTile(
              label: '音质',
              description: '会员等级不够时网易云会自动降档。',
              control: ComboBox<String>(
                value: s.quality,
                items: [
                  for (final option in loaded.qualityOptions)
                    ComboBoxItem(value: option.value, child: Text(option.label)),
                ],
                onChanged: (value) {
                  if (value != null) c.saveSettings({'quality': value});
                },
              ),
            ),
            _SettingTile(
              label: '下载并发数',
              description: '同时下载的歌曲数，越大越占带宽。',
              control: ComboBox<int>(
                value: s.maxConcurrency.toInt(),
                items: [
                  for (var i = 1; i <= 8; i++)
                    ComboBoxItem(value: i, child: Text('$i')),
                ],
                onChanged: (value) {
                  if (value != null) c.saveSettings({'maxConcurrency': value});
                },
              ),
            ),
            _SettingTile(
              label: '文件名模板',
              description: '可用占位符：{artist}、{title}、{album}、{index}。回车保存。',
              control: TextBox(
                controller: _templateController,
                textAlign: TextAlign.right,
                placeholder: '{artist} - {title}',
                onSubmitted: (value) {
                  final trimmed = value.trim();
                  if (trimmed.isEmpty || trimmed == s.fileNameTemplate) {
                    _templateController.text = s.fileNameTemplate;
                    return;
                  }
                  c.saveSettings({'fileNameTemplate': trimmed});
                },
              ),
            ),
            _SwitchTile(
              label: '写入封面',
              description: '把专辑封面写进音频文件的标签。',
              value: s.writeCover,
              onChanged: (v) => c.saveSettings({'writeCover': v}),
            ),
            _SwitchTile(
              label: '跳过已存在的文件',
              description: '目标路径上已有文件时不再下载。',
              value: s.skipExisting,
              onChanged: (v) => c.saveSettings({'skipExisting': v}),
            ),
            _SwitchTile(
              label: '覆盖已存在的文件',
              description: '与「跳过」二选一，同时开启时以覆盖为准。',
              value: s.overwriteExisting,
              onChanged: (v) => c.saveSettings({'overwriteExisting': v}),
            ),
          ],
        ),
        _Section(
          title: '本地曲库',
          description: '扫描本地音乐目录，和歌单对比出哪些已经在了。',
          rows: [
            _SwitchTile(
              label: '打开歌单后自动对比',
              description: '使用已有的扫描结果，不会每次都扫盘。',
              value: s.autoMatchLocal,
              onChanged: (v) => c.saveSettings({'autoMatchLocal': v}),
            ),
            _SettingTile(
              label: '匹配阈值',
              description: '标题、歌手、时长综合相似度低于该值就不算本地已有。',
              control: Row(
                children: [
                  Expanded(
                    child: Slider(
                      value: _threshold ?? s.matchThreshold.clamp(0.30, 1.00),
                      min: 0.30,
                      max: 1.00,
                      divisions: 70,
                      label: '${((_threshold ?? s.matchThreshold) * 100).round()}%',
                      // 拖动时只动本地值，松手才落盘，避免每帧写文件。
                      onChanged: (value) => setState(() => _threshold = value),
                      onChangeEnd: (value) {
                        setState(() => _threshold = null);
                        c.saveSettings({
                          'matchThreshold': double.parse(value.toStringAsFixed(2)),
                        });
                      },
                    ),
                  ),
                  SizedBox(
                    width: 44,
                    child: Text(
                      '${(s.matchThreshold * 100).round()}%',
                      textAlign: TextAlign.right,
                      style: const TextStyle(fontFeatures: [FontFeature.tabularFigures()]),
                    ),
                  ),
                ],
              ),
            ),
          ],
        ),
        Card(
          padding: const EdgeInsets.fromLTRB(20, 16, 20, 16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('目录', style: FluentTheme.of(context).typography.bodyStrong),
              const SizedBox(height: 4),
              _DirectoryRow(
                label: '下载目录',
                value: s.downloadDirectory,
                onBrowse: () =>
                    _pickDirectory(c, key: 'downloadDirectory', current: s.downloadDirectory),
              ),
              _DirectoryRow(
                label: '曲库目录',
                value: s.libraryDirectory,
                onBrowse: () =>
                    _pickDirectory(c, key: 'libraryDirectory', current: s.libraryDirectory),
              ),
              InfoRow(label: '数据目录', value: loaded.dataDir, copyable: true),
              InfoRow(label: '设置文件', value: loaded.settingsFile, copyable: true, monospace: true),
              const SizedBox(height: 8),
              Text(
                '下载目录与曲库目录可以用系统对话框选择；数据目录由启动参数或程序所在位置决定，改不了。',
                style: TextStyle(color: res.textFillColorSecondary, fontSize: 12),
              ),
            ],
          ),
        ),
        const SizedBox(height: 12),
        InfoBar(
          title: Text(c.settingsSaving ? '正在保存' : '设置会立即保存'),
          content: Text(
            c.settingsSaving
                ? '正在把改动写进设置文件…'
                : '改动直接写入 `settings.json`，下次启动继续生效；没有出现的旧外观键保持原样。',
          ),
          severity: c.settingsSaving ? InfoBarSeverity.info : InfoBarSeverity.success,
          isLong: true,
        ),
      ],
    );
  }
}

/// 目录行：路径可选中复制，右边的按钮打开系统目录对话框。
class _DirectoryRow extends StatelessWidget {
  const _DirectoryRow({
    required this.label,
    required this.value,
    required this.onBrowse,
  });

  final String label;
  final String value;
  final VoidCallback onBrowse;

  @override
  Widget build(BuildContext context) {
    final res = FluentTheme.of(context).resources;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 3),
      child: Row(
        children: [
          SizedBox(
            width: 96,
            child: Text(
              label,
              style: TextStyle(fontSize: 13, color: res.textFillColorSecondary),
            ),
          ),
          Expanded(
            child: SelectableText(value, style: const TextStyle(fontSize: 13)),
          ),
          const SizedBox(width: 8),
          Button(onPressed: onBrowse, child: const Text('浏览…')),
        ],
      ),
    );
  }
}

/// 一组设置：卡片标题 + 说明 + 若干行。
class _Section extends StatelessWidget {
  const _Section({required this.title, required this.description, required this.rows});

  final String title;
  final String description;
  final List<Widget> rows;

  @override
  Widget build(BuildContext context) {
    final res = FluentTheme.of(context).resources;
    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: Card(
        padding: const EdgeInsets.fromLTRB(20, 16, 20, 8),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(title, style: FluentTheme.of(context).typography.bodyStrong),
            const SizedBox(height: 2),
            Text(description, style: TextStyle(color: res.textFillColorSecondary, fontSize: 12)),
            const SizedBox(height: 4),
            for (final row in rows) row,
          ],
        ),
      ),
    );
  }
}

/// Win11 设置页的一行：左边是标题与说明，右边是控件。
class _SettingTile extends StatelessWidget {
  const _SettingTile({
    required this.label,
    required this.description,
    required this.control,
  });

  final String label;
  final String description;
  final Widget control;

  @override
  Widget build(BuildContext context) {
    final res = FluentTheme.of(context).resources;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 10),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(label),
                const SizedBox(height: 2),
                Text(
                  description,
                  style: TextStyle(color: res.textFillColorSecondary, fontSize: 12),
                ),
              ],
            ),
          ),
          const SizedBox(width: 24),
          SizedBox(width: 240, child: Align(alignment: Alignment.centerRight, child: control)),
        ],
      ),
    );
  }
}

/// 开关行：标题与说明在左，开关在右。
class _SwitchTile extends StatelessWidget {
  const _SwitchTile({
    required this.label,
    required this.description,
    required this.value,
    required this.onChanged,
  });

  final String label;
  final String description;
  final bool value;
  final ValueChanged<bool> onChanged;

  @override
  Widget build(BuildContext context) {
    final res = FluentTheme.of(context).resources;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 10),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(label),
                const SizedBox(height: 2),
                Text(
                  description,
                  style: TextStyle(color: res.textFillColorSecondary, fontSize: 12),
                ),
              ],
            ),
          ),
          const SizedBox(width: 24),
          ToggleSwitch(checked: value, onChanged: onChanged),
        ],
      ),
    );
  }
}
