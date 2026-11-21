import 'dart:async';

import 'package:fluent_ui/fluent_ui.dart';
import 'package:lunatic_princess/format.dart';
import 'package:lunatic_princess/src/rust/api/ncm.dart';

/// 打开「导入并解密 NCM」窗口。
Future<void> showNcmImportDialog(BuildContext context) {
  return showDialog<void>(
    context: context,
    barrierDismissible: false,
    builder: (_) => const _NcmImportDialog(),
  );
}

const double _headerHeight = 30;
const double _rowHeight = 38;

/// 列表里的一行：扫描到的文件 + 它现在的处理状态。
class _NcmRow {
  _NcmRow(this.file);

  final NcmFileDto file;
  NcmFileResultDto? result;

  /// 还没出结果时状态列显示的说明（待处理 / 正在解密… / 已取消）。
  String? note;

  /// 与桥接层回传的路径对齐用的键。
  String get key => _pathKey(file.path);
}

/// 路径比较不区分大小写、分隔符统一（与 `lp-core` 的去重口径一致）。
String _pathKey(String path) => path.replaceAll('/', '\\').toLowerCase();

/// 一行的状态文案与配色。
({String text, bool problem, bool success}) _rowStatus(_NcmRow row) {
  final result = row.result;
  if (result == null) {
    return (text: row.note ?? '待处理', problem: false, success: false);
  }
  if (!result.success) {
    final reason = result.errorMessage;
    return (
      text: reason == null || reason.isEmpty ? '失败' : '失败：$reason',
      problem: true,
      success: false,
    );
  }
  final parts = <String>[];
  final format = result.format;
  parts.add(format == null || format.isEmpty ? '已解密' : '已解密 · ${format.toUpperCase()}');
  final name = result.outputFileName;
  if (name != null && name.isNotEmpty) {
    parts.add('→ $name');
  }
  final enrichment = result.enrichmentMessage;
  if (enrichment != null && enrichment.isNotEmpty) {
    parts.add(enrichment);
  }
  return (text: parts.join(' '), problem: false, success: true);
}

class _NcmImportDialog extends StatefulWidget {
  const _NcmImportDialog();

  @override
  State<_NcmImportDialog> createState() => _NcmImportDialogState();
}

class _NcmImportDialogState extends State<_NcmImportDialog> {
  StreamSubscription<NcmProgressDto>? _progressSub;
  final TextEditingController _folderInput = TextEditingController();
  final TextEditingController _output = TextEditingController();

  List<String> _folders = const [];
  List<_NcmRow> _rows = const [];
  NcmProgressDto? _progress;
  bool _loading = true;
  bool _scanning = false;
  bool _running = false;
  bool _cancelling = false;
  bool _overwrite = false;
  bool _autoMatch = true;
  bool _problem = false;
  bool _onlineAvailable = true;
  String _onlineHint = '';
  String _status = '选择包含 .ncm 文件的文件夹后，这里会列出找到的文件。';
  int _scanToken = 0;

  bool get _busy => _loading || _scanning || _running;

  @override
  void initState() {
    super.initState();
    _progressSub = watchNcmProgress().listen(_onProgress);
    unawaited(_load());
  }

  @override
  void dispose() {
    _progressSub?.cancel();
    _folderInput.dispose();
    _output.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final state = await ncmGetState();
      if (!mounted) return;
      setState(() {
        _folders = state.folders;
        _output.text = state.outputDirectory;
        _onlineAvailable = state.onlineMatchAvailable;
        _onlineHint = state.onlineMatchHint;
        _loading = false;
      });
      await _scan();
    } catch (error) {
      if (!mounted) return;
      setState(() {
        _loading = false;
        _problem = true;
        _status = '读取 NCM 导入设置失败：$error';
      });
    }
  }

  Future<void> _scan() async {
    final token = ++_scanToken;
    if (_folders.isEmpty) {
      setState(() {
        _rows = const [];
        _progress = null;
        _problem = false;
        _status = '请选择包含 .ncm 文件的文件夹';
      });
      return;
    }
    setState(() {
      _scanning = true;
      _problem = false;
      _status = '正在查找 .ncm 文件…';
    });
    try {
      final result = await ncmScan(folders: List<String>.from(_folders));
      if (!mounted || token != _scanToken) return;
      setState(() {
        _scanning = false;
        _rows = result.files.map(_NcmRow.new).toList();
        _progress = null;
        _status = _scanStatus(result);
      });
    } catch (error) {
      if (!mounted || token != _scanToken) return;
      setState(() {
        _scanning = false;
        _problem = true;
        _status = '扫描 .ncm 文件失败：$error';
      });
    }
  }

  String _scanStatus(NcmScanDto result) {
    final suffix = _scanSuffix(result);
    if (result.files.isEmpty) {
      return '所选文件夹中没有找到 .ncm 文件$suffix';
    }
    return '已发现 ${result.files.length} 个 .ncm 文件，点击「开始解密」$suffix';
  }

  String _scanSuffix(NcmScanDto result) {
    final parts = <String>[];
    if (result.missingFolders.isNotEmpty) {
      parts.add('${result.missingFolders.length} 个文件夹不存在');
    }
    if (result.skippedDirectories > 0) {
      parts.add('${result.skippedDirectories} 个子文件夹无法访问已跳过');
    }
    if (parts.isEmpty) return '';
    return '（${parts.join('，')}）';
  }

  Future<void> _addFolder() async {
    final text = _folderInput.text.trim();
    if (text.isEmpty) {
      setState(() {
        _problem = true;
        _status = '请输入要查找的文件夹路径。';
      });
      return;
    }
    if (_folders.any((folder) => _pathKey(folder) == _pathKey(text))) {
      setState(() {
        _problem = true;
        _status = '这个文件夹已经在列表里。';
      });
      return;
    }
    setState(() {
      _folders = [..._folders, text];
      _folderInput.clear();
    });
    await _saveState();
    await _scan();
  }

  Future<void> _removeFolder(String folder) async {
    setState(() {
      _folders = _folders.where((item) => item != folder).toList();
    });
    await _saveState();
    await _scan();
  }

  /// 记住输入文件夹与输出目录；失败只影响下次打开时的默认值，不打断当前操作。
  Future<void> _saveState() async {
    try {
      await ncmSaveState(
        folders: List<String>.from(_folders),
        outputDirectory: _output.text.trim(),
      );
    } catch (_) {
      // 记忆只是便利功能。
    }
  }

  void _onProgress(NcmProgressDto progress) {
    if (!mounted) return;
    setState(() {
      _progress = progress;
      final key = _pathKey(progress.path);
      for (final row in _rows) {
        if (row.key != key) continue;
        final result = progress.result;
        if (progress.stage == 'finished' && result != null) {
          row.result = result;
          row.note = null;
        } else {
          row.note = progress.message.isEmpty ? '处理中…' : progress.message;
        }
        break;
      }
      _problem = false;
      if (progress.message.isNotEmpty) {
        _status = '正在处理 ${progress.index + 1}/${progress.total}：${progress.fileName} · ${progress.message}';
      } else if (_running) {
        _status = '已处理 ${progress.completed}/${progress.total}：${progress.fileName}';
      }
    });
  }

  Future<void> _start() async {
    final paths = _rows.map((row) => row.file.path).toList();
    setState(() {
      _running = true;
      _cancelling = false;
      _problem = false;
      _progress = null;
      for (final row in _rows) {
        row.result = null;
        row.note = '待处理';
      }
      _status = '开始解密 ${paths.length} 个文件…';
    });
    await _saveState();
    try {
      final summary = await ncmImport(
        request: NcmImportRequestDto(
          paths: paths,
          folders: List<String>.from(_folders),
          outputDirectory: _output.text.trim(),
          overwrite: _overwrite,
          autoMatchOnline: _autoMatch,
        ),
      );
      if (!mounted) return;
      setState(() {
        _running = false;
        _progress = null;
        final byPath = <String, NcmFileResultDto>{
          for (final result in summary.results) _pathKey(result.path): result,
        };
        for (final row in _rows) {
          final result = byPath[row.key];
          if (result != null) {
            row.result = result;
            row.note = null;
          } else if (row.result == null) {
            row.note = '已取消';
          }
        }
        _status = _summaryStatus(summary);
        _problem = summary.failed > 0;
      });
    } catch (error) {
      if (!mounted) return;
      setState(() {
        _running = false;
        _problem = true;
        _status = '导入失败：$error';
      });
    }
  }

  String _summaryStatus(NcmSummaryDto summary) {
    final matched = summary.matchedOnline > 0 ? ' · 在线匹配 ${summary.matchedOnline} 个' : '';
    if (summary.cancelled) {
      return '导入已取消：已处理 ${summary.results.length}/${summary.total}，成功 ${summary.succeeded} 个，失败 ${summary.failed} 个$matched';
    }
    return '解密完成：成功 ${summary.succeeded} 个，失败 ${summary.failed} 个（共 ${summary.total} 个）$matched';
  }

  Future<void> _cancel() async {
    setState(() {
      _cancelling = true;
      _status = '正在取消…当前文件处理完后停止';
    });
    try {
      await ncmCancel();
    } catch (_) {
      // 没有任务在跑时返回 false，不需要打扰用户。
    }
  }

  Future<void> _requestClose() async {
    if (_running) {
      try {
        await ncmCancel();
      } catch (_) {
        // 关窗时取消失败也不该拦住用户。
      }
    }
    await _saveState();
    if (mounted) Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final theme = FluentTheme.of(context);
    final size = MediaQuery.sizeOf(context);
    final width = (size.width - 64).clamp(720.0, 1100.0);
    final height = (size.height - 64).clamp(480.0, 820.0);
    return Center(
      child: SizedBox(
        width: width,
        height: height,
        child: Card(
          padding: EdgeInsets.zero,
          child: Column(
            children: [
              _buildHeader(theme),
              const Divider(style: DividerThemeData(horizontalMargin: EdgeInsets.zero)),
              Expanded(child: _buildBody(theme)),
              const Divider(style: DividerThemeData(horizontalMargin: EdgeInsets.zero)),
              _buildFooter(theme),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildHeader(FluentThemeData theme) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 14, 12, 12),
      child: Row(
        children: [
          Text('导入并解密 NCM', style: theme.typography.subtitle),
          const Spacer(),
          IconButton(
            icon: const WindowsIcon(WindowsIcons.cancel, size: 12),
            onPressed: () => unawaited(_requestClose()),
          ),
        ],
      ),
    );
  }

  Widget _buildBody(FluentThemeData theme) {
    final resources = theme.resources;
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 0, 20, 12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            '递归查找本地的 .ncm 文件，解密为 MP3 / FLAC，写入文件内自带标签与封面；可选自动在线匹配标签、封面与歌词。',
            style: TextStyle(fontSize: 12, color: resources.textFillColorSecondary),
          ),
          const SizedBox(height: 12),
          _buildFolderSection(theme),
          const SizedBox(height: 12),
          _buildOutputSection(theme),
          const SizedBox(height: 10),
          _buildOptions(theme),
          const SizedBox(height: 10),
          _buildStatus(theme),
          const SizedBox(height: 12),
          Expanded(child: _buildTable(theme)),
        ],
      ),
    );
  }

  Widget _buildFolderSection(FluentThemeData theme) {
    final resources = theme.resources;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            const Text('NCM 文件位置', style: TextStyle(fontWeight: FontWeight.w600)),
            const SizedBox(width: 8),
            Text(
              '可以添加多个文件夹，导入时会去重',
              style: TextStyle(fontSize: 12, color: resources.textFillColorSecondary),
            ),
          ],
        ),
        const SizedBox(height: 6),
        Row(
          children: [
            Expanded(
              child: TextBox(
                controller: _folderInput,
                enabled: !_busy,
                placeholder: '文件夹路径，例如 D:/音乐/NCM',
                onSubmitted: (_) => unawaited(_addFolder()),
              ),
            ),
            const SizedBox(width: 8),
            Button(
              onPressed: _busy ? null : () => unawaited(_addFolder()),
              child: const Text('添加文件夹'),
            ),
          ],
        ),
        if (_folders.isNotEmpty) ...[
          const SizedBox(height: 6),
          ConstrainedBox(
            constraints: const BoxConstraints(maxHeight: 96),
            child: DecoratedBox(
              decoration: BoxDecoration(
                border: Border.all(color: resources.dividerStrokeColorDefault),
                borderRadius: BorderRadius.circular(6),
              ),
              child: ListView.builder(
                shrinkWrap: true,
                itemCount: _folders.length,
                itemBuilder: (context, index) {
                  final folder = _folders[index];
                  return Padding(
                    padding: const EdgeInsets.fromLTRB(10, 2, 4, 2),
                    child: Row(
                      children: [
                        Expanded(
                          child: Text(
                            folder,
                            style: const TextStyle(fontSize: 12),
                            maxLines: 1,
                            overflow: TextOverflow.ellipsis,
                          ),
                        ),
                        IconButton(
                          icon: const WindowsIcon(WindowsIcons.remove, size: 12),
                          onPressed: _busy ? null : () => unawaited(_removeFolder(folder)),
                        ),
                      ],
                    ),
                  );
                },
              ),
            ),
          ),
        ],
      ],
    );
  }

  Widget _buildOutputSection(FluentThemeData theme) {
    final resources = theme.resources;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        const Text('输出目录', style: TextStyle(fontWeight: FontWeight.w600)),
        const SizedBox(height: 6),
        TextBox(
          controller: _output,
          enabled: !_busy,
          placeholder: '解密后的文件放在哪里，例如 D:/音乐/解密',
          onSubmitted: (_) => unawaited(_saveState()),
        ),
        const SizedBox(height: 4),
        Text(
          '需要是完整路径；目录不存在会创建。',
          style: TextStyle(fontSize: 11.5, color: resources.textFillColorSecondary),
        ),
      ],
    );
  }

  Widget _buildOptions(FluentThemeData theme) {
    final resources = theme.resources;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Checkbox(
          checked: _overwrite,
          onChanged: _busy ? null : (value) => setState(() => _overwrite = value ?? false),
          content: const Text('覆盖已存在的解密文件'),
        ),
        Checkbox(
          checked: _autoMatch,
          onChanged: _busy ? null : (value) => setState(() => _autoMatch = value ?? false),
          content: const Text('解密后自动获取在线标签、封面与歌词'),
        ),
        if (_autoMatch && _onlineHint.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(left: 28, top: 2),
            child: Text(
              _onlineHint,
              style: TextStyle(
                fontSize: 11.5,
                color: _onlineAvailable ? resources.textFillColorSecondary : resources.systemFillColorCaution,
              ),
            ),
          ),
      ],
    );
  }

  Widget _buildStatus(FluentThemeData theme) {
    final resources = theme.resources;
    final progress = _progress;
    final percent = progress == null || progress.total <= 0
        ? null
        : (progress.completed / progress.total * 100).clamp(0, 100).toDouble();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          _status,
          style: TextStyle(
            fontSize: 12,
            color: _problem ? resources.systemFillColorCaution : resources.textFillColorSecondary,
          ),
        ),
        if (percent != null) ...[
          const SizedBox(height: 6),
          ProgressBar(value: percent, strokeWidth: 3),
        ],
      ],
    );
  }

  Widget _buildTable(FluentThemeData theme) {
    final resources = theme.resources;
    return DecoratedBox(
      decoration: BoxDecoration(
        border: Border.all(color: resources.dividerStrokeColorDefault),
        borderRadius: BorderRadius.circular(10),
      ),
      child: Column(
        children: [
          SizedBox(
            height: _headerHeight,
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 10),
              child: Row(
                children: [
                  Expanded(flex: 4, child: _headerText(theme, '文件')),
                  SizedBox(width: 90, child: _headerText(theme, '大小')),
                  Expanded(flex: 6, child: _headerText(theme, '状态')),
                ],
              ),
            ),
          ),
          const Divider(
            style: DividerThemeData(horizontalMargin: EdgeInsets.zero),
          ),
          Expanded(
            child: _rows.isEmpty
                ? Center(
                    child: Text(
                      _scanning ? '正在查找 .ncm 文件…' : '还没有文件。添加包含 .ncm 的文件夹后会在这里列出。',
                      style: TextStyle(fontSize: 13, color: resources.textFillColorSecondary),
                    ),
                  )
                : ListView.builder(
                    itemExtent: _rowHeight,
                    itemCount: _rows.length,
                    itemBuilder: (context, index) => _buildRow(theme, index),
                  ),
          ),
        ],
      ),
    );
  }

  Widget _headerText(FluentThemeData theme, String label) {
    return Align(
      alignment: Alignment.centerLeft,
      child: Text(
        label,
        style: TextStyle(
          fontSize: 11.5,
          fontWeight: FontWeight.w600,
          color: theme.resources.textFillColorSecondary,
        ),
      ),
    );
  }

  Widget _buildRow(FluentThemeData theme, int index) {
    final row = _rows[index];
    final status = _rowStatus(row);
    final resources = theme.resources;
    final color = status.problem
        ? resources.systemFillColorCritical
        : status.success
            ? const Color(0xFF0F7B0F)
            : resources.textFillColorSecondary;
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 4),
        child: Row(
          children: [
            Expanded(
              flex: 4,
              child: Text(
                row.file.name,
                style: const TextStyle(fontSize: 12, fontWeight: FontWeight.w600),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            ),
            SizedBox(
              width: 90,
              child: Text(
                formatFileSize(row.file.size),
                style: TextStyle(fontSize: 12, color: resources.textFillColorSecondary),
              ),
            ),
            Expanded(
              flex: 6,
              child: Text(
                status.text,
                style: TextStyle(fontSize: 12, color: color),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildFooter(FluentThemeData theme) {
    final resources = theme.resources;
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 12, 20, 14),
      child: Row(
        children: [
          Text(
            _rows.isEmpty ? '没有文件' : '已发现 ${_rows.length} 个文件',
            style: TextStyle(fontSize: 11.5, color: resources.textFillColorSecondary),
          ),
          const Spacer(),
          if (_running)
            FilledButton(
              onPressed: _cancelling ? null : () => unawaited(_cancel()),
              child: Text(_cancelling ? '正在取消…' : '取消'),
            )
          else
            FilledButton(
              onPressed: _busy || _rows.isEmpty ? null : () => unawaited(_start()),
              child: const Text('开始解密'),
            ),
          const SizedBox(width: 8),
          Button(
            onPressed: () => unawaited(_requestClose()),
            child: const Text('关闭'),
          ),
        ],
      ),
    );
  }
}
