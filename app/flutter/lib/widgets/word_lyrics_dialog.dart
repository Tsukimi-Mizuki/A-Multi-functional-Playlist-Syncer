import 'dart:async';

import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter/services.dart';
import 'package:lunatic_princess/file_dialogs.dart';
import 'package:lunatic_princess/src/rust/api/lyrics.dart';
import 'package:lunatic_princess/src/rust/api/tags.dart';

/// 「歌词转逐字」：先联网检查哪些歌能换（不改文件），确认后才写入。
Future<void> showWordLyricsDialog(
  BuildContext context, {
  required String initialFolder,
}) {
  return showDialog<void>(
    context: context,
    barrierDismissible: false,
    builder: (context) => _WordLyricsDialog(initialFolder: initialFolder),
  );
}

enum _Phase { idle, scanning, checking, writing }

class _WordRow {
  _WordRow(this.file);

  final BatchFileDto file;

  /// 检查 / 转换返回的结果；还没处理时为 null。
  WordLyricsResultDto? result;

  /// 处理过程中的临时说明（等待处理 / 正在核对… / 已取消）。
  String? note;
}

String _pathKey(String path) => path.replaceAll('/', r'\').toLowerCase();

String _directoryOf(String path) {
  final index = path.lastIndexOf(RegExp(r'[\\/]'));
  return index <= 0 ? '' : path.substring(0, index);
}

String _statusLabel(String status) => switch (status) {
      'ready' => '可以转换',
      'converted' => '已转换',
      'alreadyWord' => '已经是逐字',
      'noLyrics' => '没有歌词',
      'noMatch' => '没找到这首歌',
      'noWordLyrics' => '没有逐字版本',
      'mismatch' => '版本对不上',
      'failed' => '失败',
      _ => status,
    };

class _WordLyricsDialog extends StatefulWidget {
  const _WordLyricsDialog({required this.initialFolder});

  final String initialFolder;

  @override
  State<_WordLyricsDialog> createState() => _WordLyricsDialogState();
}

class _WordLyricsDialogState extends State<_WordLyricsDialog> {
  static const double _rowHeight = 34;
  static const double _headerHeight = 30;
  static const double _listPadding = 8;

  final TextEditingController _folder = TextEditingController();
  final FocusNode _listFocus = FocusNode();

  StreamSubscription<WordLyricsProgressDto>? _progressSub;

  List<_WordRow> _rows = <_WordRow>[];
  Set<String> _selected = <String>{};
  int _current = -1;
  int _anchor = -1;
  double _viewportHeight = 0;
  bool _recursive = true;

  _Phase _phase = _Phase.idle;
  WordLyricsProgressDto? _progress;
  String _status = '选择音乐目录，或把音频文件、文件夹拖进窗口。';
  bool _warning = false;
  bool _cancelling = false;
  int _listSeq = 0;

  bool get _working => _phase == _Phase.checking || _phase == _Phase.writing;

  bool get _busy => _working || _phase == _Phase.scanning;

  List<_WordRow> get _targets {
    if (_selected.isEmpty) return _rows;
    return _rows
        .where((row) => _selected.contains(_pathKey(row.file.path)))
        .toList();
  }

  @override
  void initState() {
    super.initState();
    _folder.text = widget.initialFolder.trim();
    _progressSub = watchWordLyricsProgress().listen(_onProgress);
    if (_folder.text.isNotEmpty) {
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) unawaited(_scan());
      });
    }
  }

  @override
  void dispose() {
    _progressSub?.cancel();
    if (_working) unawaited(wordLyricsCancel());
    _listFocus.dispose();
    _folder.dispose();
    super.dispose();
  }

  void _onProgress(WordLyricsProgressDto progress) {
    if (!mounted) return;
    setState(() {
      _progress = progress;
      final result = progress.result;
      if (result != null) _applyResult(result);
    });
  }

  void _applyResult(WordLyricsResultDto result) {
    final key = _pathKey(result.filePath);
    for (final row in _rows) {
      if (_pathKey(row.file.path) == key) {
        row.result = result;
        row.note = null;
        return;
      }
    }
  }

  /// 用系统目录对话框挑要处理的目录，选好立刻扫描。
  Future<void> _pickFolder() async {
    final picked = await pickDirectory(initialDirectory: _folder.text.trim());
    if (picked == null || !mounted) return;
    setState(() => _folder.text = picked);
    await _scan();
  }

  Future<void> _scan() async {
    final folder = _folder.text.trim();
    final seq = ++_listSeq;
    setState(() {
      _phase = _Phase.scanning;
      _progress = null;
      _warning = false;
      _status = '正在扫描音频文件...';
    });
    if (folder.isEmpty) {
      setState(() {
        _phase = _Phase.idle;
        _rows = <_WordRow>[];
        _selected = <String>{};
        _current = -1;
        _anchor = -1;
        _status = '请选择有效的音乐目录。';
        _warning = true;
      });
      return;
    }
    try {
      final scan = await batchScan(folder: folder, recursive: _recursive);
      if (!mounted || seq != _listSeq) return;
      setState(() {
        _phase = _Phase.idle;
        _progress = null;
        _selected = <String>{};
        _current = -1;
        _anchor = -1;
        if (scan.directoryMissing) {
          _rows = <_WordRow>[];
          _status = '请选择有效的音乐目录。';
          _warning = true;
          return;
        }
        _rows = scan.files.map(_WordRow.new).toList();
        if (_rows.isEmpty) {
          _status = '目录中没有找到支持的音频文件。';
          _warning = true;
          return;
        }
        _status = '共 ${_rows.length} 首。可以先选中要处理的歌曲（不选就是全部），再点「检查并转换」。';
        _warning = false;
      });
    } catch (error) {
      if (!mounted || seq != _listSeq) return;
      setState(() {
        _phase = _Phase.idle;
        _status = '扫描目录失败：$error';
        _warning = true;
      });
    }
  }

  String _summaryText(WordLyricsSummaryDto summary) {
    final counts = <String, int>{};
    for (final result in summary.results) {
      counts[result.status] = (counts[result.status] ?? 0) + 1;
    }
    const order = <String>[
      'ready',
      'converted',
      'alreadyWord',
      'noLyrics',
      'noMatch',
      'noWordLyrics',
      'mismatch',
      'failed',
    ];
    final parts = <String>[];
    for (final status in order) {
      final count = counts[status] ?? 0;
      if (count > 0) parts.add('${_statusLabel(status)} $count 首');
    }
    return parts.isEmpty ? '没有处理任何歌曲' : parts.join('，');
  }

  Future<bool> _confirmWrite(int count, String text) async {
    final result = await showDialog<bool>(
      context: context,
      builder: (context) => ContentDialog(
        title: const Text('把歌词换成逐字歌词'),
        content: SizedBox(
          width: 480,
          child: Text(
            '$text。要把这 $count 首的歌词换成逐字歌词吗？'
            '内嵌在音频文件里的歌词会写进音频文件；歌词文件（.lrc）会被改写，'
            '原文件先备份到它所在目录的 _backup 文件夹。现有的译文会保留，其他标签不动。',
          ),
        ),
        actions: <Widget>[
          Button(
            onPressed: () => Navigator.of(context).pop(false),
            child: const Text('先不转换'),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: Text('转换 $count 首'),
          ),
        ],
      ),
    );
    return result ?? false;
  }

  Future<void> _start() async {
    final targets = _targets;
    if (_phase != _Phase.idle || targets.isEmpty) return;
    final paths = targets.map((row) => row.file.path).toList();
    setState(() {
      _phase = _Phase.checking;
      _cancelling = false;
      _progress = null;
      _warning = false;
      _status = '正在检查歌词能否换成逐字… 0/${paths.length}';
      for (final row in targets) {
        row.result = null;
        row.note = '等待处理';
      }
    });

    WordLyricsOutcomeDto outcome;
    try {
      outcome = await wordLyricsPreview(files: paths);
    } catch (error) {
      if (!mounted) return;
      setState(() {
        _phase = _Phase.idle;
        _status = '检查歌词失败：$error';
        _warning = true;
      });
      return;
    }
    if (!mounted) return;
    final error = outcome.error;
    final summary = outcome.summary;
    if (error != null || summary == null) {
      setState(() {
        _phase = _Phase.idle;
        _status = '检查歌词失败：${error?.message ?? '没有返回结果'}';
        _warning = true;
      });
      return;
    }
    final byKey = <String, WordLyricsResultDto>{
      for (final result in summary.results) _pathKey(result.filePath): result,
    };
    final text = _summaryText(summary);
    setState(() {
      for (final row in targets) {
        final result = byKey[_pathKey(row.file.path)];
        if (result != null) {
          row.result = result;
          row.note = null;
        } else if (row.result == null) {
          row.note = '已取消';
        }
      }
      _phase = _Phase.idle;
      _progress = null;
      final lead = summary.cancelled
          ? '已取消检查（处理了 ${summary.processed}/${summary.total} 首）'
          : '检查完成';
      _warning = summary.failed > 0 || summary.cancelled;
      _status = summary.ready == 0
          ? '$lead，没有可以换成逐字歌词的歌曲：$text。'
          : '$lead：$text';
    });

    if (summary.ready == 0 || !mounted) return;
    final confirmed = await _confirmWrite(summary.ready, text);
    if (!mounted) return;
    if (!confirmed) {
      setState(() {
        _status = '没有转换，文件都没有改动。各首歌的检查结果留在列表里。';
      });
      return;
    }
    await _write(paths);
  }

  Future<void> _write(List<String> paths) async {
    setState(() {
      _phase = _Phase.writing;
      _cancelling = false;
      _progress = null;
      _warning = false;
      _status = '正在把歌词换成逐字… 0/${paths.length}';
    });
    WordLyricsOutcomeDto outcome;
    try {
      outcome = await wordLyricsApply(files: paths);
    } catch (error) {
      if (!mounted) return;
      setState(() {
        _phase = _Phase.idle;
        _status = '歌词转逐字失败：$error';
        _warning = true;
      });
      return;
    }
    if (!mounted) return;
    final error = outcome.error;
    final summary = outcome.summary;
    if (error != null || summary == null) {
      setState(() {
        _phase = _Phase.idle;
        _status = '歌词转逐字失败：${error?.message ?? '没有返回结果'}';
        _warning = true;
      });
      return;
    }
    for (final result in summary.results) {
      _applyResult(result);
    }
    setState(() {
      _phase = _Phase.idle;
      _progress = null;
      _warning = summary.failed > 0 || summary.cancelled;
      final lead = summary.cancelled
          ? '已取消（处理了 ${summary.processed}/${summary.total} 首）：'
          : '歌词转逐字完成：';
      _status = '$lead${_summaryText(summary)}。';
    });
  }

  Future<void> _cancel() async {
    if (!_working || _cancelling) return;
    setState(() {
      _cancelling = true;
      _status = '正在取消…当前文件处理完后停止';
    });
    try {
      await wordLyricsCancel();
    } catch (_) {
      // 取消失败也不影响界面：任务自己会结束。
    }
  }

  Future<void> _requestClose() async {
    if (_working) {
      try {
        await wordLyricsCancel();
      } catch (_) {
        // 同上，关窗优先。
      }
    }
    if (!mounted) return;
    ++_listSeq;
    Navigator.of(context).pop();
  }

  Future<void> _applySelection(
    Set<String> keys, {
    required int anchor,
    required int current,
  }) async {
    setState(() {
      _selected = keys;
      _anchor = anchor;
      _current = current;
    });
  }

  Set<String> _rangeSelection(int from, int to) {
    final start = from < to ? from : to;
    final end = from < to ? to : from;
    return <String>{
      for (var i = start; i <= end; i += 1) _pathKey(_rows[i].file.path),
    };
  }

  Future<void> _selectRow(int index) async {
    if (index < 0 || index >= _rows.length) return;
    final keyboard = HardwareKeyboard.instance;
    final key = _pathKey(_rows[index].file.path);
    if (keyboard.isShiftPressed) {
      final anchor = _anchor < 0 ? index : _anchor;
      await _applySelection(
        _rangeSelection(anchor, index),
        anchor: anchor,
        current: index,
      );
      return;
    }
    if (keyboard.isControlPressed) {
      final next = <String>{..._selected};
      if (!next.remove(key)) next.add(key);
      await _applySelection(next, anchor: index, current: index);
      return;
    }
    await _applySelection(<String>{key}, anchor: index, current: index);
  }

  Future<void> _selectAll() async {
    await _applySelection(
      <String>{for (final row in _rows) _pathKey(row.file.path)},
      anchor: _anchor,
      current: _current,
    );
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent && event is! KeyRepeatEvent) {
      return KeyEventResult.ignored;
    }
    final keyboard = HardwareKeyboard.instance;
    final key = event.logicalKey;
    if (keyboard.isControlPressed && key == LogicalKeyboardKey.keyA) {
      unawaited(_selectAll());
      return KeyEventResult.handled;
    }
    if (_rows.isEmpty || _busy) return KeyEventResult.ignored;
    final page = (((_viewportHeight - _headerHeight) / _rowHeight).floor() - 1)
        .clamp(1, 999);
    int? target;
    if (key == LogicalKeyboardKey.arrowDown) {
      target = (_current < 0 ? 0 : _current + 1).clamp(0, _rows.length - 1);
    } else if (key == LogicalKeyboardKey.arrowUp) {
      target = (_current <= 0 ? 0 : _current - 1).clamp(0, _rows.length - 1);
    } else if (key == LogicalKeyboardKey.pageDown) {
      target = ((_current < 0 ? 0 : _current) + page).clamp(0, _rows.length - 1);
    } else if (key == LogicalKeyboardKey.pageUp) {
      target = ((_current < 0 ? 0 : _current) - page).clamp(0, _rows.length - 1);
    } else if (key == LogicalKeyboardKey.home) {
      target = 0;
    } else if (key == LogicalKeyboardKey.end) {
      target = _rows.length - 1;
    }
    if (target == null) return KeyEventResult.ignored;
    if (keyboard.isShiftPressed) {
      final anchor = _anchor < 0 ? target : _anchor;
      unawaited(
        _applySelection(
          _rangeSelection(anchor, target),
          anchor: anchor,
          current: target,
        ),
      );
    } else {
      unawaited(_applySelection(<String>{_pathKey(_rows[target].file.path)},
          anchor: target, current: target));
    }
    return KeyEventResult.handled;
  }

  String _rowStatus(_WordRow row) {
    final result = row.result;
    if (result != null) {
      final message = result.message.trim();
      return message.isEmpty ? _statusLabel(result.status) : message;
    }
    return row.note ?? '';
  }

  Color? _rowColor(FluentThemeData theme, _WordRow row) {
    final status = row.result?.status;
    if (status == 'ready' || status == 'converted') {
      return const Color(0xFF0F7B0F);
    }
    if (status == 'failed') return theme.resources.systemFillColorCritical;
    return theme.resources.textFillColorSecondary;
  }

  @override
  Widget build(BuildContext context) {
    final theme = FluentTheme.of(context);
    final size = MediaQuery.sizeOf(context);
    final width = (size.width - 64).clamp(720.0, 1240.0);
    final height = (size.height - 64).clamp(480.0, 940.0);
    return CallbackShortcuts(
      bindings: <ShortcutActivator, VoidCallback>{
        const SingleActivator(LogicalKeyboardKey.escape): () {
          unawaited(_requestClose());
        },
      },
      child: Focus(
        autofocus: true,
        child: Center(
          child: SizedBox(
            width: width,
            height: height,
            child: Card(
              padding: EdgeInsets.zero,
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: <Widget>[
                  _buildHeader(theme),
                  const Divider(style: DividerThemeData(horizontalMargin: EdgeInsets.zero)),
                  _buildSource(theme),
                  if (_progress != null && (_progress?.total ?? 0) > 0)
                    ProgressBar(
                      value: (_progress!.completed / _progress!.total) * 100,
                      strokeWidth: 3,
                    ),
                  Expanded(child: _buildList(theme)),
                  const Divider(style: DividerThemeData(horizontalMargin: EdgeInsets.zero)),
                  _buildFooter(theme),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  Widget _buildHeader(FluentThemeData theme) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 14, 12, 12),
      child: Row(
        children: <Widget>[
          Text('歌词转逐字', style: theme.typography.subtitle),
          const Spacer(),
          IconButton(
            icon: const WindowsIcon(WindowsIcons.cancel, size: 12),
            onPressed: () => unawaited(_requestClose()),
          ),
        ],
      ),
    );
  }

  Widget _buildSource(FluentThemeData theme) {
    final resources = theme.resources;
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 0, 20, 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: <Widget>[
          Row(
            children: <Widget>[
              Expanded(
                child: TextBox(
                  controller: _folder,
                  placeholder: '选择包含音频文件的目录，或把文件、文件夹拖进窗口',
                  enabled: !_busy,
                  onSubmitted: (_) => unawaited(_scan()),
                ),
              ),
              const SizedBox(width: 8),
              Button(
                onPressed: _busy ? null : () => unawaited(_pickFolder()),
                child: const Text('浏览…'),
              ),
              const SizedBox(width: 8),
              Button(
                onPressed: _busy ? null : () => unawaited(_scan()),
                child: const Row(
                  mainAxisSize: MainAxisSize.min,
                  children: <Widget>[
                    WindowsIcon(WindowsIcons.refresh, size: 14),
                    SizedBox(width: 6),
                    Text('刷新'),
                  ],
                ),
              ),
            ],
          ),
          const SizedBox(height: 8),
          Row(
            children: <Widget>[
              Checkbox(
                checked: _recursive,
                onChanged: _busy
                    ? null
                    : (value) {
                        setState(() => _recursive = value ?? false);
                        unawaited(_scan());
                      },
                content: const Text('包含子文件夹'),
              ),
              const SizedBox(width: 16),
              if (_selected.isNotEmpty)
                HyperlinkButton(
                  onPressed: _busy
                      ? null
                      : () => _applySelection(
                            <String>{},
                            anchor: _anchor,
                            current: _current,
                          ),
                  child: const Text('取消选择（处理全部）'),
                ),
            ],
          ),
          const SizedBox(height: 6),
          Text(
            '把现有的逐行歌词换成逐字歌词：先联网检查哪些能换（不改文件），确认后才写入。'
            '已经是逐字的、没有歌词的不动；原有的译文会保留；同名 .lrc 改写前会备份到 _backup 文件夹。',
            style: TextStyle(fontSize: 12, color: resources.textFillColorSecondary),
          ),
        ],
      ),
    );
  }

  Widget _buildList(FluentThemeData theme) {
    final resources = theme.resources;
    return Padding(
      padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
      child: LayoutBuilder(
        builder: (context, constraints) {
          _viewportHeight = constraints.maxHeight;
          return DecoratedBox(
            decoration: BoxDecoration(
              border: Border.all(color: resources.dividerStrokeColorDefault),
              borderRadius: BorderRadius.circular(10),
            ),
            child: Focus(
              focusNode: _listFocus,
              onKeyEvent: _onKey,
              child: _rows.isEmpty
                  ? Center(
                      child: Text(
                        '还没有歌曲。选择音乐目录，或把音频文件、文件夹拖进窗口。',
                        style: TextStyle(
                          fontSize: 13,
                          color: resources.textFillColorSecondary,
                        ),
                      ),
                    )
                  : Column(
                      crossAxisAlignment: CrossAxisAlignment.stretch,
                      children: <Widget>[
                        _buildListHeader(theme),
                        const Divider(
                          style: DividerThemeData(horizontalMargin: EdgeInsets.zero),
                        ),
                        Expanded(
                          child: ListView.builder(
                            itemExtent: _rowHeight,
                            itemCount: _rows.length,
                            itemBuilder: (context, index) => _buildRow(theme, index),
                          ),
                        ),
                      ],
                    ),
            ),
          );
        },
      ),
    );
  }

  Widget _buildListHeader(FluentThemeData theme) {
    return SizedBox(
      height: _headerHeight,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: _listPadding),
        child: Row(
          children: <Widget>[
            Expanded(
              flex: 13,
              child: Text(
                '文件名',
                style: TextStyle(
                  fontSize: 11.5,
                  color: theme.resources.textFillColorSecondary,
                ),
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              flex: 15,
              child: Text(
                '状态',
                style: TextStyle(
                  fontSize: 11.5,
                  color: theme.resources.textFillColorSecondary,
                ),
              ),
            ),
            const SizedBox(width: 12),
            Expanded(
              flex: 10,
              child: Text(
                '所在目录',
                style: TextStyle(
                  fontSize: 11.5,
                  color: theme.resources.textFillColorSecondary,
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildRow(FluentThemeData theme, int index) {
    final resources = theme.resources;
    final row = _rows[index];
    final key = _pathKey(row.file.path);
    final selected = _selected.contains(key);
    final current = _current == index && _listFocus.hasFocus;
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () {
          _listFocus.requestFocus();
          unawaited(_selectRow(index));
        },
        child: Container(
          margin: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
          padding: const EdgeInsets.symmetric(horizontal: 6),
          decoration: BoxDecoration(
            color: selected ? resources.subtleFillColorSecondary : null,
            border: current ? Border.all(color: theme.accentColor) : null,
            borderRadius: BorderRadius.circular(4),
          ),
          child: Row(
            children: <Widget>[
              Expanded(
                flex: 13,
                child: Text(
                  row.file.name,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(
                    fontSize: 12,
                    fontWeight: FontWeight.w600,
                  ),
                ),
              ),
              const SizedBox(width: 12),
              Expanded(
                flex: 15,
                child: Text(
                  _rowStatus(row),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(fontSize: 12, color: _rowColor(theme, row)),
                ),
              ),
              const SizedBox(width: 12),
              Expanded(
                flex: 10,
                child: Text(
                  _directoryOf(row.file.path),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 12,
                    color: resources.textFillColorSecondary,
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildFooter(FluentThemeData theme) {
    final resources = theme.resources;
    final targets = _targets.length;
    final total = _rows.length;
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 12, 20, 14),
      child: Row(
        children: <Widget>[
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: <Widget>[
                Text(
                  total == 0
                      ? '没有歌曲'
                      : '已选 ${_selected.length} / 共 $total 首',
                  style: TextStyle(
                    fontSize: 11.5,
                    color: resources.textFillColorSecondary,
                    fontFeatures: const <FontFeature>[FontFeature.tabularFigures()],
                  ),
                ),
                Text(
                  _status,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 12,
                    color: _warning
                        ? resources.systemFillColorCaution
                        : resources.textFillColorPrimary,
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(width: 12),
          if (_working)
            Button(
              onPressed: _cancelling ? null : () => unawaited(_cancel()),
              child: Text(_cancelling ? '正在取消…' : '取消'),
            )
          else
            FilledButton(
              onPressed: _phase == _Phase.idle && targets > 0
                  ? () => unawaited(_start())
                  : null,
              child: Text(
                _selected.isEmpty
                    ? '检查并转换（全部 $total 首）'
                    : '检查并转换（选中的 $targets 首）',
              ),
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
