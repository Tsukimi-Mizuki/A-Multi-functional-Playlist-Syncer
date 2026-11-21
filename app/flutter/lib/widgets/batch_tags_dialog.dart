import 'dart:async';

import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter/services.dart';
import 'package:lunatic_princess/src/rust/api/batch.dart';
import 'package:lunatic_princess/src/rust/api/online.dart';
import 'package:lunatic_princess/src/rust/api/tags.dart';
import 'package:lunatic_princess/widgets/tag_editor_dialog.dart';

/// 「保持」占位：选中的歌曲里这一项的值不一致，不动它就原样保留。
const String _kKeepHint = '〈保持〉';

/// 「清空」占位：这一项被改动过、输入框留空，保存时清除。
const String _kClearHint = '〈清空〉';

/// 批量面板的字段：`name` 要和 [TagEditDto.field] 的 camelCase 名一致。
class _FieldSpec {
  const _FieldSpec(this.name, this.label, {this.numeric = false});

  final String name;
  final String label;

  /// 数字字段：留空=清除，非纯数字由后端拒绝并给出中文原因。
  final bool numeric;
}

const List<_FieldSpec> _fields = <_FieldSpec>[
  _FieldSpec('title', '标题'),
  _FieldSpec('artist', '艺术家'),
  _FieldSpec('album', '专辑'),
  _FieldSpec('year', '年份', numeric: true),
  _FieldSpec('trackNumber', '音轨号', numeric: true),
  _FieldSpec('discNumber', '碟号', numeric: true),
  _FieldSpec('genre', '风格'),
  _FieldSpec('albumArtist', '专辑艺术家'),
  _FieldSpec('composer', '作曲家'),
  _FieldSpec('lyricist', '作词家'),
  _FieldSpec('comment', '注释'),
];

enum _Phase { idle, scanning, reading, saving, online }

/// 列表里的一行：路径与文件名先有，标签是分块读回来的。
class _BatchRow {
  _BatchRow({required this.path, required this.name, required this.size});

  final String path;
  final String name;
  final int size;
  TagRowDto? row;

  /// 保存或在线获取后的逐行标记（已保存 / 在线补全 / 失败原因）。
  String? status;

  /// 上一次在线获取是否成功（成功行的状态文案用成功色）。
  bool onlineOk = false;

  bool get readable => row?.readable ?? false;

  bool get failed => row != null && !(row?.readable ?? false);

  bool get saved => status == '已保存';

  String get directory {
    final index = path.lastIndexOf(RegExp(r'[\\/]'));
    return index <= 0 ? path : path.substring(0, index);
  }

  String get statusText {
    if (status != null) return status!;
    if (row == null) return '正在读取…';
    if (readable) return '';
    return row?.error ?? '无法读取标签';
  }
}

/// 路径比较键：Windows 不区分大小写，`/` 与 `\\` 等价。
String _pathKey(String path) => path.replaceAll('/', r'\').toLowerCase();

/// 列表列宽：与 Tauri 版一致，总宽超过窗口时横向滚动。
class _ColumnSpec {
  const _ColumnSpec(this.label, this.width, this.value);

  final String label;
  final double width;
  final String Function(_BatchRow row) value;
}

final List<_ColumnSpec> _columns = <_ColumnSpec>[
  _ColumnSpec('文件名', 230, (row) => row.name),
  _ColumnSpec('状态', 180, (row) => row.statusText),
  _ColumnSpec('目录', 170, (row) => row.directory),
  _ColumnSpec('格式', 58, (row) => row.row?.fileFormat ?? ''),
  _ColumnSpec('标题', 180, (row) => row.row?.title ?? ''),
  _ColumnSpec('艺术家', 150, (row) => row.row?.artist ?? ''),
  _ColumnSpec('专辑', 170, (row) => row.row?.album ?? ''),
  _ColumnSpec('专辑艺术家', 130, (row) => row.row?.albumArtist ?? ''),
  _ColumnSpec('年份', 56, (row) => row.row?.year?.toString() ?? ''),
  _ColumnSpec('音轨号', 56, (row) => row.row?.trackNumber?.toString() ?? ''),
  _ColumnSpec('碟号', 50, (row) => row.row?.discNumber?.toString() ?? ''),
  _ColumnSpec('风格', 100, (row) => row.row?.genre ?? ''),
  _ColumnSpec('作曲家', 130, (row) => row.row?.composer ?? ''),
  _ColumnSpec('作词家', 130, (row) => row.row?.lyricist ?? ''),
];

const double _cellGap = 12;
const double _listPadding = 12;
const double _rowHeight = 34;
const double _headerHeight = 30;

double get _tableWidth =>
    _columns.fold<double>(0, (sum, column) => sum + column.width) +
    _cellGap * (_columns.length - 1) +
    _listPadding * 2;

/// 打开批量修改标签面板。
///
/// 目前没有拖放与文件对话框（M7/M8 再补），所以清单只能按目录列出：
/// [initialFolder] 一般传设置里的曲库目录，或当前编辑文件所在的目录。
Future<void> showBatchTagsDialog(
  BuildContext context, {
  required String initialFolder,
  bool recursive = true,
}) {
  return showDialog<void>(
    context: context,
    barrierDismissible: false,
    builder: (_) =>
        _BatchTagsDialog(initialFolder: initialFolder, recursive: recursive),
  );
}

class _BatchTagsDialog extends StatefulWidget {
  const _BatchTagsDialog({required this.initialFolder, required this.recursive});

  final String initialFolder;
  final bool recursive;

  @override
  State<_BatchTagsDialog> createState() => _BatchTagsDialogState();
}

class _BatchTagsDialogState extends State<_BatchTagsDialog> {
  final TextEditingController _folderController = TextEditingController();
  final Map<String, TextEditingController> _controllers =
      <String, TextEditingController>{
        for (final field in _fields) field.name: TextEditingController(),
      };
  final FocusNode _listFocus = FocusNode(debugLabel: 'batch-list');

  bool _recursive = true;
  List<_BatchRow> _rows = <_BatchRow>[];
  Set<int> _selected = <int>{};
  int _anchor = -1;
  int _current = -1;
  Map<String, String> _edits = <String, String>{};
  bool _coverRemoved = false;
  _Phase _phase = _Phase.idle;
  String _status = '选择音乐目录，然后点「刷新」列出音频文件。';
  bool _statusWarning = false;
  int _seq = 0;
  int _readDone = 0;
  int _readTotal = 0;
  int _saveDone = 0;
  int _saveTotal = 0;
  bool _cancelRequested = false;
  bool _onlineCancelRequested = false;
  bool _overwrite = false;
  bool _fetchLyrics = true;
  bool _preferWord = false;
  String _tagPriorityText = '网易云音乐优先';
  String _lyricsPriorityText = '网易云音乐优先';
  String _thresholdPercent = '60';
  int _onlineDone = 0;
  int _onlineTotal = 0;
  StreamSubscription<BatchProgressDto>? _onlineSub;
  double _viewportHeight = 320;

  @override
  void initState() {
    super.initState();
    _folderController.text = widget.initialFolder;
    _recursive = widget.recursive;
    unawaited(_loadOnlineOptions());
    if (widget.initialFolder.trim().isNotEmpty) {
      unawaited(_scan());
    }
  }

  @override
  void dispose() {
    unawaited(_onlineSub?.cancel() ?? Future<void>.value());
    if (_phase == _Phase.online) {
      unawaited(batchCancel());
    }
    _folderController.dispose();
    for (final controller in _controllers.values) {
      controller.dispose();
    }
    _listFocus.dispose();
    super.dispose();
  }

  bool get _busy => _phase == _Phase.saving || _phase == _Phase.online;

  bool get _loading => _phase == _Phase.scanning || _phase == _Phase.reading;

  bool get _dirty => _edits.isNotEmpty || _coverRemoved;

  List<_BatchRow> get _selectedRows {
    final indexes = _selected.toList()..sort();
    return <_BatchRow>[
      for (final index in indexes)
        if (index >= 0 && index < _rows.length) _rows[index],
    ];
  }

  List<_BatchRow> get _writableRows =>
      _selectedRows.where((row) => row.readable).toList(growable: false);

  int get _unwritableCount => _selectedRows.length - _writableRows.length;

  /// 某个字段在选中行里的共同值；值不同时 `mixed` 为真、`value` 为空。
  ({String value, bool mixed}) _common(String fieldName) {
    final rows = _writableRows;
    if (rows.isEmpty) return (value: '', mixed: false);
    final first = _rowValue(rows.first, fieldName);
    final mixed = rows.any((row) => _rowValue(row, fieldName) != first);
    return (value: mixed ? '' : first, mixed: mixed);
  }

  String _rowValue(_BatchRow target, String fieldName) {
    final row = target.row;
    switch (fieldName) {
      case 'title':
        return row?.title ?? '';
      case 'artist':
        return row?.artist ?? '';
      case 'album':
        return row?.album ?? '';
      case 'year':
        return row?.year?.toString() ?? '';
      case 'trackNumber':
        return row?.trackNumber?.toString() ?? '';
      case 'discNumber':
        return row?.discNumber?.toString() ?? '';
      case 'genre':
        return row?.genre ?? '';
      case 'albumArtist':
        return row?.albumArtist ?? '';
      case 'composer':
        return row?.composer ?? '';
      case 'lyricist':
        return row?.lyricist ?? '';
      case 'comment':
        return row?.comment ?? '';
    }
    return '';
  }

  /// 换选择后把左面板恢复到「没改过」的状态。
  void _resetPanel() {
    _edits = <String, String>{};
    _coverRemoved = false;
    for (final field in _fields) {
      _controllers[field.name]!.text = _common(field.name).value;
    }
  }

  Future<void> _scan() async {
    if (_busy) return;
    final seq = ++_seq;
    setState(() {
      _phase = _Phase.scanning;
      _status = '正在扫描音频文件...';
      _statusWarning = false;
      _rows = <_BatchRow>[];
      _selected = <int>{};
      _anchor = -1;
      _current = -1;
      _readDone = 0;
      _readTotal = 0;
      _resetPanel();
    });
    BatchScanDto scan;
    try {
      scan = await batchScan(
        folder: _folderController.text.trim(),
        recursive: _recursive,
      );
    } catch (error) {
      if (!mounted || seq != _seq) return;
      setState(() {
        _phase = _Phase.idle;
        _status = '扫描目录失败：$error';
        _statusWarning = true;
      });
      return;
    }
    if (!mounted || seq != _seq) return;
    if (scan.directoryMissing) {
      setState(() {
        _phase = _Phase.idle;
        _status = '请选择有效的音乐目录。';
        _statusWarning = true;
      });
      return;
    }
    if (scan.files.isEmpty) {
      setState(() {
        _phase = _Phase.idle;
        _status = '目录中没有找到支持的音频文件。';
        _statusWarning = true;
      });
      return;
    }
    final rows = <_BatchRow>[
      for (final file in scan.files)
        _BatchRow(path: file.path, name: file.name, size: file.size),
    ];
    setState(() {
      _rows = rows;
      _phase = _Phase.reading;
      _readDone = 0;
      _readTotal = rows.length;
      _status = '正在读取标签… 0/${rows.length}';
    });
    await _readRows(rows, seq);
  }

  /// 分块读标签（100 个文件一块）：先到先显示，单行读不出来不影响其它行。
  Future<void> _readRows(List<_BatchRow> rows, int seq) async {
    const int chunkSize = 100;
    for (var start = 0; start < rows.length; start += chunkSize) {
      final end = (start + chunkSize).clamp(0, rows.length);
      final slice = rows.sublist(start, end);
      List<TagRowDto> dtos;
      try {
        dtos = await readTagRows(
          paths: <String>[for (final row in slice) row.path],
        );
      } catch (error) {
        if (!mounted || seq != _seq) return;
        setState(() {
          _phase = _Phase.idle;
          _status = '读取标签失败：$error';
          _statusWarning = true;
        });
        return;
      }
      if (!mounted || seq != _seq) return;
      setState(() {
        for (var i = 0; i < dtos.length && i < slice.length; i += 1) {
          slice[i].row = dtos[i];
        }
        _readDone = end;
        _status = '正在读取标签… $_readDone/$_readTotal';
      });
    }
    if (!mounted || seq != _seq) return;
    setState(() {
      _phase = _Phase.idle;
      _status = '共 $_readTotal 首。选中歌曲后在左边修改标签。';
      _statusWarning = false;
    });
  }

  Future<bool> _confirmDiscard(String message, String confirmLabel) async {
    final result = await showDialog<bool>(
      context: context,
      builder: (context) => ContentDialog(
        title: const Text('有未保存的修改'),
        content: Text(message),
        actions: <Widget>[
          Button(
            onPressed: () => Navigator.of(context).pop(false),
            child: const Text('继续编辑'),
          ),
          FilledButton(
            onPressed: () => Navigator.of(context).pop(true),
            child: Text(confirmLabel),
          ),
        ],
      ),
    );
    return result ?? false;
  }

  Future<void> _applySelection(
    Set<int> next, {
    required int anchor,
    required int current,
  }) async {
    if (_busy) return;
    if (_dirty && !_sameIndexes(next, _selected)) {
      final ok = await _confirmDiscard(
        '当前修改尚未保存，换成别的歌曲会丢弃这些修改。',
        '放弃修改并切换',
      );
      if (!ok) return;
    } else if (_sameIndexes(next, _selected)) {
      setState(() {
        _anchor = anchor;
        _current = current;
      });
      return;
    }
    setState(() {
      _selected = next;
      _anchor = anchor;
      _current = current;
      _resetPanel();
    });
  }

  bool _sameIndexes(Set<int> a, Set<int> b) =>
      a.length == b.length && a.containsAll(b);

  Set<int> _rangeSelection(int from, int to) {
    final start = from < 0 ? to : from;
    final low = start < to ? start : to;
    final high = start < to ? to : start;
    return <int>{for (var i = low; i <= high; i += 1) i};
  }

  Future<void> _selectRow(int index) async {
    final keyboard = HardwareKeyboard.instance;
    final control = keyboard.isControlPressed;
    final shift = keyboard.isShiftPressed;
    if (shift) {
      final anchor = _anchor < 0 ? index : _anchor;
      await _applySelection(
        _rangeSelection(anchor, index),
        anchor: anchor,
        current: index,
      );
      return;
    }
    if (control && _selected.contains(index)) {
      final next = Set<int>.from(_selected)..remove(index);
      await _applySelection(next, anchor: index, current: index);
      return;
    }
    await _applySelection(<int>{index}, anchor: index, current: index);
  }

  Future<void> _selectAll() async {
    if (_busy || _rows.isEmpty) return;
    await _applySelection(
      <int>{for (var i = 0; i < _rows.length; i += 1) i},
      anchor: 0,
      current: _current < 0 ? 0 : _current,
    );
  }

  void _onFieldChanged(String fieldName, String text) {
    final common = _common(fieldName);
    final next = Map<String, String>.from(_edits);
    if (!common.mixed && text == common.value) {
      next.remove(fieldName);
    } else {
      next[fieldName] = text;
    }
    setState(() => _edits = next);
  }

  void _restoreField(String fieldName) {
    setState(() {
      _edits = Map<String, String>.from(_edits)..remove(fieldName);
      _controllers[fieldName]!.text = _common(fieldName).value;
    });
  }

  Future<void> _save() async {
    if (!_dirty || _busy || _phase == _Phase.reading) return;
    final targets = _writableRows;
    if (targets.isEmpty) return;
    final edits = <TagEditDto>[
      for (final entry in _edits.entries)
        TagEditDto(field: entry.key, value: entry.value),
    ];
    final cover = CoverChangeDto(kind: _coverRemoved ? 'remove' : 'keep');
    setState(() {
      _phase = _Phase.saving;
      _cancelRequested = false;
      _saveDone = 0;
      _saveTotal = targets.length;
      _status = '正在保存标签… 0/${targets.length}';
      _statusWarning = false;
      for (final row in targets) {
        row.status = null;
      }
    });

    const int chunkSize = 20;
    var saved = 0;
    var failed = 0;
    String? firstFailure;
    var cancelled = false;
    for (var start = 0; start < targets.length; start += chunkSize) {
      if (_cancelRequested) {
        cancelled = true;
        break;
      }
      final slice = targets.sublist(
        start,
        (start + chunkSize).clamp(0, targets.length),
      );
      TagPatchOutcomeDto outcome;
      try {
        outcome = await patchTags(
          paths: <String>[for (final row in slice) row.path],
          edits: edits,
          cover: cover,
        );
      } catch (error) {
        if (!mounted) return;
        setState(() {
          _phase = _Phase.idle;
          _status = '保存失败：$error';
          _statusWarning = true;
        });
        return;
      }
      if (!mounted) return;
      final error = outcome.error;
      if (error != null) {
        setState(() {
          _phase = _Phase.idle;
          _status = '保存失败：${error.message}';
          _statusWarning = true;
        });
        return;
      }
      final results = outcome.results ?? const <TagPatchResultDto>[];
      final byPath = <String, _BatchRow>{
        for (final row in _rows) _pathKey(row.path): row,
      };
      setState(() {
        for (final result in results) {
          final row = byPath[_pathKey(result.path)];
          if (row == null) continue;
          row.row = result.row;
          if (result.success) {
            saved += 1;
            row.status = '已保存';
          } else {
            failed += 1;
            final message = result.message ?? '保存失败，请重试。';
            firstFailure ??= message;
            row.status = message;
          }
        }
        _saveDone = (start + slice.length).clamp(0, _saveTotal);
        _status = '正在保存标签… $_saveDone/$_saveTotal';
      });
    }

    if (!mounted) return;
    final remaining = _saveTotal - _saveDone;
    setState(() {
      _phase = _Phase.idle;
      if (cancelled) {
        _status = '已取消保存：已保存 $saved 首，还有 $remaining 首没有处理。';
        _statusWarning = true;
      } else if (failed > 0) {
        _status = '已保存 $saved 首，失败 $failed 首：$firstFailure';
        _statusWarning = true;
      } else {
        _status = '已保存 $saved 首。';
        _statusWarning = false;
        _resetPanel();
      }
    });
  }

  void _cancelSave() {
    if (_phase != _Phase.saving) return;
    setState(() {
      _cancelRequested = true;
      _status = '正在取消…当前这一批保存完后停止';
      _statusWarning = true;
    });
  }

  /// 取设置里的来源优先级与匹配度阈值，给在线获取那一行写说明。
  Future<void> _loadOnlineOptions() async {
    try {
      final options = await editorOptions();
      if (!mounted) return;
      setState(() {
        _tagPriorityText = options.tagPriorityText;
        _lyricsPriorityText = options.lyricsPriorityText;
        _thresholdPercent = options.thresholdPercent;
      });
    } catch (_) {
      // 说明文字取不到不影响任务本身，用默认文案即可。
    }
  }

  /// 在线获取的进度（逐文件）。
  void _onOnlineProgress(BatchProgressDto event) {
    if (!mounted || _phase != _Phase.online) return;
    final index = _rows.indexWhere(
      (row) => _pathKey(row.path) == _pathKey(event.path),
    );
    setState(() {
      if (index >= 0) {
        final row = _rows[index];
        final result = event.result;
        if (event.stage == 'finished' && result != null) {
          _applyOnlineResult(row, result);
        } else {
          row.status = '正在匹配…';
        }
      }
      _onlineDone = event.completed;
      _onlineTotal = event.total;
      _status = '正在在线获取标签、封面与歌词… $_onlineDone/$_onlineTotal';
    });
  }

  void _applyOnlineResult(_BatchRow row, BatchResultDto result) {
    row.status = result.message;
    row.onlineOk = result.success;
    if (result.success) {
      row.onlineOk = true;
    }
  }

  /// 批量在线获取：先匹配再写标签与封面，逐文件推进度。
  Future<void> _runOnline() async {
    if (_phase != _Phase.idle || _dirty) return;
    final targets = _selectedRows.isEmpty ? _rows : _selectedRows;
    final paths = <String>[];
    final seen = <String>{};
    for (final row in targets) {
      if (seen.add(_pathKey(row.path))) paths.add(row.path);
    }
    if (paths.isEmpty) {
      setState(() {
        _status = '没有要在线获取的歌曲。';
        _statusWarning = true;
      });
      return;
    }
    await _onlineSub?.cancel();
    _onlineSub = watchBatchProgress().listen(_onOnlineProgress);
    setState(() {
      _phase = _Phase.online;
      _onlineDone = 0;
      _onlineTotal = paths.length;
      _onlineCancelRequested = false;
      _status = '正在在线获取标签、封面与歌词… 0/${paths.length}';
      _statusWarning = false;
      for (final row in targets) {
        row.status = '等待处理';
        row.onlineOk = false;
      }
    });

    BatchRunOutcomeDto outcome;
    try {
      outcome = await batchRun(
        request: BatchRunRequestDto(
          files: paths,
          overwriteExisting: _overwrite,
          fetchLyrics: _fetchLyrics,
          preferWordLyrics: _preferWord,
        ),
      );
    } catch (error) {
      await _onlineSub?.cancel();
      _onlineSub = null;
      if (!mounted) return;
      setState(() {
        _phase = _Phase.idle;
        _status = '在线获取失败：$error';
        _statusWarning = true;
      });
      return;
    }
    await _onlineSub?.cancel();
    _onlineSub = null;
    if (!mounted) return;
    final failure = outcome.error;
    final summary = outcome.summary;
    if (failure != null || summary == null) {
      setState(() {
        _phase = _Phase.idle;
        _status = '在线获取失败：${failure?.message ?? '没有返回结果，请重试。'}';
        _statusWarning = true;
      });
      return;
    }
    final byPath = <String, _BatchRow>{
      for (final row in _rows) _pathKey(row.path): row,
    };
    final refreshed = <String>[];
    setState(() {
      for (final result in summary.results) {
        final row = byPath[_pathKey(result.filePath)];
        if (row == null) continue;
        _applyOnlineResult(row, result);
        if (result.success) refreshed.add(result.filePath);
      }
      for (final row in targets) {
        if (row.status == '等待处理' || row.status == '正在匹配…') {
          row.status = '已取消';
          row.onlineOk = false;
        }
      }
      _onlineDone = summary.processed;
      _onlineTotal = summary.total;
      _phase = _Phase.idle;
      _statusWarning = summary.failed > 0;
      _status = summary.cancelled
          ? '已取消在线获取：已处理 ${summary.processed}/${summary.total}。'
          : '在线获取完成：成功 ${summary.success}，跳过 ${summary.skipped}，失败 ${summary.failed}。';
    });

    // 写成功的行重新读一遍标签，左边面板和列表跟着刷新。
    if (refreshed.isNotEmpty) {
      try {
        final rows = await readTagRows(paths: refreshed);
        if (!mounted) return;
        setState(() {
          for (final dto in rows) {
            final row = byPath[_pathKey(dto.path)];
            if (row != null) row.row = dto;
          }
        });
      } catch (_) {
        // 读不回来时保留任务给出的状态，下次刷新会重新读。
      }
    }
  }

  void _cancelOnline() {
    if (_phase != _Phase.online) return;
    setState(() {
      _onlineCancelRequested = true;
      _status = '正在取消…当前文件处理完后停止';
      _statusWarning = true;
    });
    unawaited(batchCancel());
  }
  Future<void> _requestClose() async {
    if (_phase == _Phase.online) {
      unawaited(batchCancel());
    }
    if (_dirty) {
      final ok = await _confirmDiscard(
        '当前修改尚未保存，关闭后会丢失这些修改。',
        '放弃修改并关闭',
      );
      if (!ok) return;
    }
    if (!mounted) return;
    ++_seq;
    Navigator.of(context).pop();
  }

  void _openEditor() {
    final rows = _writableRows;
    if (rows.length != 1) return;
    final path = rows.first.path;
    if (_dirty) {
      unawaited(() async {
        final ok = await _confirmDiscard(
          '当前修改尚未保存，打开歌词与标签编辑器会丢弃这些修改。',
          '放弃修改并打开',
        );
        if (!ok || !mounted) return;
        _resetPanel();
        setState(() {});
        unawaited(showTagEditorDialog(context, path));
      }());
      return;
    }
    unawaited(showTagEditorDialog(context, path));
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
      unawaited(_applySelection(
        _rangeSelection(anchor, target),
        anchor: anchor,
        current: target,
      ));
    } else {
      unawaited(_applySelection(<int>{target}, anchor: target, current: target));
    }
    return KeyEventResult.handled;
  }

  String get _panelTitle {
    if (_selectedRows.isEmpty) return '在右边选择要修改的歌曲';
    final writable = _writableRows;
    if (writable.isEmpty) return '选中的歌曲还没有读出标签，或无法读取';
    if (writable.length == 1) return writable.first.name;
    return '已选 ${writable.length} 首：值不同的字段显示〈保持〉';
  }

  String get _lyricsPreview {
    final rows = _writableRows;
    if (rows.isEmpty) return '';
    if (rows.length > 1) {
      final withLyrics = rows.where((row) => row.row?.hasLyrics ?? false).length;
      return '${rows.length} 首中 $withLyrics 首有歌词';
    }
    final row = rows.first.row;
    final preview = row?.lyricsPreview ?? '';
    if ((row?.hasLyrics ?? false) && preview.isNotEmpty) return preview;
    final sidecar = row?.lyricsFile;
    if (sidecar != null && sidecar.isNotEmpty) {
      return '（来自同名文件「$sidecar」，音频文件里没有内嵌歌词）';
    }
    return '没有歌词';
  }

  String get _coverCaption {
    final rows = _writableRows;
    if (_coverRemoved) return '封面将在保存时移除';
    if (rows.isEmpty) return '';
    if (rows.length > 1) {
      final withCover = rows.where((row) => row.row?.hasCover ?? false).length;
      return '${rows.length} 首中 $withCover 首有封面';
    }
    return (rows.first.row?.hasCover ?? false) ? '这一首有封面，保存时保持原样' : '没有封面';
  }

  @override
  Widget build(BuildContext context) {
    final theme = FluentTheme.of(context);
    final size = MediaQuery.sizeOf(context);
    final width = (size.width - 64).clamp(720.0, 1480.0);
    final height = (size.height - 64).clamp(480.0, 940.0);
    final selectedCount = _selected.length;
    final writableCount = _writableRows.length;
    return CallbackShortcuts(
      bindings: <ShortcutActivator, VoidCallback>{
        const SingleActivator(LogicalKeyboardKey.keyS, control: true): () {
          unawaited(_save());
        },
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
                  if (_phase == _Phase.reading ||
                      _phase == _Phase.saving ||
                      _phase == _Phase.online)
                    ProgressBar(value: _progressPercent, strokeWidth: 3),
                  Expanded(
                    child: Padding(
                      padding: const EdgeInsets.fromLTRB(16, 8, 16, 8),
                      child: Row(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: <Widget>[
                          SizedBox(width: 304, child: _buildPanel(theme)),
                          const SizedBox(width: 12),
                          Expanded(child: _buildList(theme)),
                        ],
                      ),
                    ),
                  ),
                  const Divider(style: DividerThemeData(horizontalMargin: EdgeInsets.zero)),
                  _buildFooter(theme, selectedCount, writableCount),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  double? get _progressPercent {
    if (_phase == _Phase.saving) {
      return _saveTotal == 0 ? null : _saveDone / _saveTotal * 100;
    }
    if (_phase == _Phase.online) {
      return _onlineTotal == 0 ? null : _onlineDone / _onlineTotal * 100;
    }
    return _readTotal == 0 ? null : _readDone / _readTotal * 100;
  }

  Widget _buildHeader(FluentThemeData theme) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 14, 12, 12),
      child: Row(
        children: <Widget>[
          Text('批量修改标签', style: theme.typography.subtitle),
          const SizedBox(width: 12),
          if (_dirty)
            Text(
              '● 有未保存的修改',
              style: TextStyle(
                fontSize: 12,
                color: theme.resources.systemFillColorCaution,
              ),
            ),
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
    final enabled = !_busy && !_loading;
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 12, 20, 10),
      child: Row(
        children: <Widget>[
          Expanded(
            child: TextBox(
              controller: _folderController,
              enabled: enabled,
              placeholder: '包含音频文件的目录',
              onSubmitted: (_) => unawaited(_scan()),
            ),
          ),
          const SizedBox(width: 8),
          Button(
            onPressed: enabled ? () => unawaited(_scan()) : null,
            child: const Text('刷新'),
          ),
          const SizedBox(width: 12),
          Checkbox(
            checked: _recursive,
            onChanged: enabled
                ? (value) {
                    setState(() => _recursive = value ?? false);
                    unawaited(_scan());
                  }
                : null,
            content: const Text('包含子文件夹'),
          ),
        ],
      ),
    );
  }

  Widget _buildPanel(FluentThemeData theme) {
    final resources = theme.resources;
    final writable = _writableRows;
    final hasAnyCover = writable.any((row) => row.row?.hasCover ?? false);
    return DecoratedBox(
      decoration: BoxDecoration(
        border: Border.all(color: resources.dividerStrokeColorDefault),
        borderRadius: BorderRadius.circular(8),
      ),
      child: SingleChildScrollView(
        padding: const EdgeInsets.fromLTRB(12, 10, 12, 12),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: <Widget>[
            Text(
              _panelTitle,
              style: TextStyle(
                fontSize: 12,
                color: resources.textFillColorSecondary,
              ),
              maxLines: 2,
              overflow: TextOverflow.ellipsis,
            ),
            if (_unwritableCount > 0)
              Padding(
                padding: const EdgeInsets.only(top: 2),
                child: Text(
                  '（另有 $_unwritableCount 首无法修改）',
                  style: TextStyle(
                    fontSize: 12,
                    color: resources.systemFillColorCaution,
                  ),
                ),
              ),
            const SizedBox(height: 8),
            for (final field in _fields)
              Padding(
                padding: const EdgeInsets.only(bottom: 4),
                child: _fieldRow(theme, field),
              ),
            _lyricsRow(theme),
            const SizedBox(height: 10),
            Row(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: <Widget>[
                SizedBox(
                  width: 92,
                  height: 92,
                  child: DecoratedBox(
                    decoration: BoxDecoration(
                      color: resources.subtleFillColorSecondary,
                      borderRadius: BorderRadius.circular(8),
                    ),
                    child: Center(
                      child: WindowsIcon(
                        WindowsIcons.music_note,
                        size: 22,
                        color: resources.textFillColorTertiary,
                      ),
                    ),
                  ),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: <Widget>[
                      if (_coverCaption.isNotEmpty)
                        Text(
                          _coverCaption,
                          style: TextStyle(
                            fontSize: 12,
                            color: resources.textFillColorSecondary,
                          ),
                        ),
                      const SizedBox(height: 4),
                      Wrap(
                        spacing: 6,
                        runSpacing: 6,
                        children: <Widget>[
                          Button(
                            onPressed: (!_busy &&
                                    writable.isNotEmpty &&
                                    !_coverRemoved &&
                                    hasAnyCover)
                                ? () => setState(() => _coverRemoved = true)
                                : null,
                            child: const Text('移除封面'),
                          ),
                          if (_coverRemoved)
                            Button(
                              onPressed: _busy
                                  ? null
                                  : () => setState(() => _coverRemoved = false),
                              child: const Text('还原'),
                            ),
                        ],
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ],
        ),
      ),
    );
  }

  Widget _fieldRow(FluentThemeData theme, _FieldSpec field) {
    final resources = theme.resources;
    final common = _common(field.name);
    final edited = _edits.containsKey(field.name);
    final controller = _controllers[field.name]!;
    final enabled = !_busy && !_loading && _writableRows.isNotEmpty;
    final placeholder = edited
        ? _kClearHint
        : (common.mixed ? _kKeepHint : '');
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Row(
          children: <Widget>[
            Text(
              field.label,
              style: TextStyle(
                fontSize: 11,
                color: resources.textFillColorSecondary,
              ),
            ),
            if (edited) ...<Widget>[
              const Spacer(),
              Tooltip(
                message: '还原这一项',
                child: IconButton(
                  icon: const WindowsIcon(WindowsIcons.undo, size: 10),
                  onPressed: _busy ? null : () => _restoreField(field.name),
                ),
              ),
            ],
          ],
        ),
        const SizedBox(height: 2),
        TextBox(
          controller: controller,
          enabled: enabled,
          placeholder: placeholder,
          onChanged: (text) => _onFieldChanged(field.name, text),
        ),
      ],
    );
  }

  Widget _lyricsRow(FluentThemeData theme) {
    final resources = theme.resources;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        Row(
          children: <Widget>[
            Text(
              '歌词',
              style: TextStyle(
                fontSize: 11,
                color: resources.textFillColorSecondary,
              ),
            ),
            const Spacer(),
            Button(
              onPressed: (!_busy && _writableRows.length == 1)
                  ? _openEditor
                  : null,
              child: const Text('编辑…'),
            ),
          ],
        ),
        const SizedBox(height: 2),
        Container(
          height: 24,
          padding: const EdgeInsets.symmetric(horizontal: 6),
          alignment: Alignment.centerLeft,
          decoration: BoxDecoration(
            color: resources.subtleFillColorSecondary,
            borderRadius: BorderRadius.circular(8),
          ),
          child: Text(
            _lyricsPreview,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: const TextStyle(fontFamily: 'Consolas', fontSize: 12),
          ),
        ),
      ],
    );
  }

  /// 在线获取的设置行：选项只影响这一批任务，不写进设置。
  Widget _buildOnlineBar(FluentThemeData theme) {
    final resources = theme.resources;
    final enabled = _phase == _Phase.idle;
    return Row(
      children: <Widget>[
        const Text(
          '在线获取',
          style: TextStyle(fontSize: 12, fontWeight: FontWeight.w600),
        ),
        const SizedBox(width: 12),
        Checkbox(
          checked: _overwrite,
          onChanged: enabled
              ? (value) => setState(() => _overwrite = value ?? false)
              : null,
          content: const Text('覆盖已有标签与歌词'),
        ),
        const SizedBox(width: 8),
        Checkbox(
          checked: _fetchLyrics,
          onChanged: enabled
              ? (value) => setState(() => _fetchLyrics = value ?? false)
              : null,
          content: const Text('同时获取歌词'),
        ),
        const SizedBox(width: 8),
        SizedBox(
          width: 170,
          child: ComboBox<String>(
            value: _preferWord ? 'word' : 'line',
            isExpanded: true,
            onChanged: enabled && _fetchLyrics
                ? (value) => setState(() => _preferWord = value == 'word')
                : null,
            items: const <ComboBoxItem<String>>[
              ComboBoxItem(value: 'line', child: Text('逐行歌词')),
              ComboBoxItem(value: 'word', child: Text('逐字歌词（有则优先）')),
            ],
          ),
        ),
        const SizedBox(width: 12),
        Expanded(
          child: Text(
            '标签 $_tagPriorityText · 歌词 $_lyricsPriorityText · 匹配度阈值 $_thresholdPercent%（来源顺序在设置里调整；低于阈值的跳过）',
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: TextStyle(
              fontSize: 11.5,
              color: resources.textFillColorSecondary,
            ),
          ),
        ),
      ],
    );
  }
  Widget _buildList(FluentThemeData theme) {
    final resources = theme.resources;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        _buildOnlineBar(theme),
        const SizedBox(height: 8),
        Expanded(
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
                      '还没有歌曲。选择音乐目录，然后点「刷新」。',
                      style: TextStyle(
                        fontSize: 13,
                        color: resources.textFillColorSecondary,
                      ),
                    ),
                  )
                : SingleChildScrollView(
                    scrollDirection: Axis.horizontal,
                    child: SizedBox(
                      width: _tableWidth < constraints.maxWidth
                          ? constraints.maxWidth
                          : _tableWidth,
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: <Widget>[
                          _buildListHeader(theme),
                          const Divider(
                            style: DividerThemeData(
                              horizontalMargin: EdgeInsets.zero,
                            ),
                          ),
                          Expanded(
                            child: ListView.builder(
                              itemExtent: _rowHeight,
                              itemCount: _rows.length,
                              itemBuilder: (context, index) => _buildRow(
                                theme,
                                index,
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
              ),
            );
            },
          ),
        ),
      ],
    );
  }

  Widget _buildListHeader(FluentThemeData theme) {
    return SizedBox(
      height: _headerHeight,
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: _listPadding),
        child: Row(
          children: <Widget>[
            for (var i = 0; i < _columns.length; i += 1) ...<Widget>[
              if (i > 0) const SizedBox(width: _cellGap),
              SizedBox(
                width: _columns[i].width,
                child: Text(
                  _columns[i].label,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 11.5,
                    color: theme.resources.textFillColorSecondary,
                  ),
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }

  Widget _buildRow(FluentThemeData theme, int index) {
    final resources = theme.resources;
    final row = _rows[index];
    final selected = _selected.contains(index);
    final current = _current == index && _listFocus.hasFocus;
    return MouseRegion(
      cursor: SystemMouseCursors.click,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () {
          _listFocus.requestFocus();
          unawaited(_selectRow(index));
        },
        onDoubleTap: () {
          _listFocus.requestFocus();
          if (!_selected.contains(index)) {
            unawaited(_applySelection(<int>{index}, anchor: index, current: index));
          }
          if (row.readable) {
            unawaited(showTagEditorDialog(context, row.path));
          }
        },
        child: Container(
          margin: const EdgeInsets.symmetric(horizontal: 6, vertical: 1),
          padding: const EdgeInsets.symmetric(horizontal: 6),
          decoration: BoxDecoration(
            color: selected ? resources.subtleFillColorSecondary : null,
            border: current
                ? Border.all(color: theme.accentColor)
                : null,
            borderRadius: BorderRadius.circular(4),
          ),
          child: Row(
            children: <Widget>[
              for (var i = 0; i < _columns.length; i += 1) ...<Widget>[
                if (i > 0) const SizedBox(width: _cellGap),
                SizedBox(
                  width: _columns[i].width,
                  child: _cellWidget(theme, row, i),
                ),
              ],
            ],
          ),
        ),
      ),
    );
  }

  Widget _cellWidget(FluentThemeData theme, _BatchRow row, int columnIndex) {
    final resources = theme.resources;
    final text = _columns[columnIndex].value(row);
    final bold = columnIndex == 0;
    Color? color;
    if (columnIndex == 1) {
      if (row.saved || row.onlineOk) {
        color = const Color(0xFF0F7B0F);
      } else if (row.failed) {
        color = resources.systemFillColorCritical;
      } else {
        color = resources.textFillColorSecondary;
      }
    }
    return Text(
      text,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: TextStyle(
        fontSize: 12,
        fontWeight: bold ? FontWeight.w600 : FontWeight.normal,
        color: color,
        fontFeatures: const <FontFeature>[FontFeature.tabularFigures()],
      ),
    );
  }

  Widget _buildFooter(
    FluentThemeData theme,
    int selectedCount,
    int writableCount,
  ) {
    final resources = theme.resources;
    final saving = _phase == _Phase.saving;
    final online = _phase == _Phase.online;
    final onlineCount =
        _selectedRows.isEmpty ? _rows.length : _selectedRows.length;
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
                  _rows.isEmpty ? '没有歌曲' : '已选 $selectedCount / 共 ${_rows.length} 首',
                  style: TextStyle(
                    fontSize: 11.5,
                    color: resources.textFillColorSecondary,
                    fontFeatures: const <FontFeature>[
                      FontFeature.tabularFigures(),
                    ],
                  ),
                ),
                Text(
                  _status,
                  maxLines: 2,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 12,
                    color: _statusWarning
                        ? resources.systemFillColorCaution
                        : resources.textFillColorPrimary,
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(width: 12),
          if (saving)
            Button(
              onPressed: _cancelRequested ? null : _cancelSave,
              child: Text(_cancelRequested ? '正在取消…' : '取消'),
            )
          else if (online)
            Button(
              onPressed: _onlineCancelRequested ? null : _cancelOnline,
              child: Text(_onlineCancelRequested ? '正在取消…' : '取消'),
            )
          else ...<Widget>[
            Tooltip(
              message: _dirty ? '请先保存或还原左边的修改' : '联网匹配标签、封面与歌词（低于阈值的跳过）',
              child: Button(
                onPressed: (!_dirty && !_loading && onlineCount > 0)
                    ? () => unawaited(_runOnline())
                    : null,
                child: Text(
                  onlineCount > _selectedRows.length
                      ? '在线获取（全部 $onlineCount 首）'
                      : '在线获取（选中的 $onlineCount 首）',
                ),
              ),
            ),
            const SizedBox(width: 8),
            FilledButton(
              onPressed: (_dirty && writableCount > 0 && !_loading)
                  ? () => unawaited(_save())
                  : null,
              child: Text(writableCount > 1 ? '保存到 $writableCount 首' : '保存修改'),
            ),
          ],
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
