import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter/services.dart';
import 'package:lunatic_princess/controller.dart';
import 'package:lunatic_princess/src/rust/api/download.dart';
import 'package:lunatic_princess/src/rust/api/playlists.dart';

/// 没有可下载项时的提示（与 Tauri 版文案一致）。
const String kNothingMissing =
    '没有需要下载的歌曲：本地已存在，或是待解密的 NCM（可以用「一键同步」解密）。';

/// 任务汇总的一行中文文案。
String describeTransfer(TransferSummaryDto s) {
  final parts = <String>[
    '共 ${s.total} 首',
    '成功 ${s.succeeded}',
    '下载 ${s.downloaded}',
    '解密 ${s.decrypted}',
    '跳过 ${s.skipped}',
    if (s.failed > 0) '失败 ${s.failed}',
  ];
  return parts.join('  ·  ');
}

/// 只用来显示一条消息的对话框。
Future<void> showTransferMessage(
  BuildContext context,
  String title,
  String message,
) {
  return showDialog<void>(
    context: context,
    builder: (ctx) => ContentDialog(
      title: Text(title),
      content: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 520),
        child: Text(message),
      ),
      actions: [
        Button(onPressed: () => Navigator.pop(ctx), child: const Text('知道了')),
      ],
    ),
  );
}

/// 选中的歌曲里各状态的数量（与 lp-core 的 plan_transfer 同一套判定）。
({int available, int ncm, int missing}) selectedCounts(AppController c) {
  final songs = c.detail?.songs ?? const <SongDto>[];
  final matches = c.match?.matches;
  var available = 0;
  var ncm = 0;
  var missing = 0;
  for (final row in c.selectedRows) {
    if (row >= songs.length) continue;
    final availability = matches != null && row < matches.length
        ? matches[row].availability
        : 'missing';
    switch (availability) {
      case 'available':
        available++;
      case 'ncm':
        ncm++;
      default:
        missing++;
    }
  }
  return (available: available, ncm: ncm, missing: missing);
}

/// 「下载选中」：先说明选中项里哪些会被跳过，再开始任务。
Future<void> showDownloadSelectedDialog(
  BuildContext context,
  AppController c,
) async {
  if (c.selectedRows.isEmpty) return;
  final counts = selectedCounts(c);
  final total = c.selectedRows.length;
  final confirmed = await showDialog<bool>(
    context: context,
    builder: (ctx) => ContentDialog(
      title: const Text('下载选中的歌曲'),
      content: SizedBox(
        width: 460,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              '已选中 $total 首：本地已有 ${counts.available} 首会跳过，'
              '${counts.ncm} 首是待解密的 NCM，${counts.missing} 首要下载。',
            ),
            if (counts.ncm > 0) ...[
              const SizedBox(height: 8),
              const Text('NCM 文件由「一键同步」解密，直接下载会跳过它们。'),
            ],
            if (counts.missing == 0 && counts.ncm == 0) ...[
              const SizedBox(height: 8),
              const Text(kNothingMissing),
            ],
            const SizedBox(height: 12),
            Text(
              '下载到：${c.appState?.downloadDirectory ?? '默认目录'}',
              style: const TextStyle(fontSize: 12),
            ),
          ],
        ),
      ),
      actions: [
        Button(onPressed: () => Navigator.pop(ctx, false), child: const Text('取消')),
        FilledButton(
          onPressed: () => Navigator.pop(ctx, true),
          child: const Text('开始下载'),
        ),
      ],
    ),
  );
  if (confirmed != true || !context.mounted) return;
  await c.downloadSelectedTracks();
}

/// 「下载缺失」：列出缺失行、默认全选，可 Shift 连选。
Future<void> showDownloadMissingDialog(
  BuildContext context,
  AppController c,
) async {
  final result = await c.requestTransferPlan();
  if (!context.mounted) return;
  if (result == null) {
    final error = c.transferError;
    if (error != null) await showTransferMessage(context, '无法下载', error);
    return;
  }
  final error = result.error;
  if (error != null) {
    await showTransferMessage(context, '无法下载', error.message);
    return;
  }
  final plan = result.plan;
  if (plan == null) return;
  if (plan.missingRows.isEmpty) {
    await showTransferMessage(context, '下载缺失', kNothingMissing);
    return;
  }
  final songs = c.detail?.songs ?? const <SongDto>[];
  final ids = await showDialog<List<int>>(
    context: context,
    builder: (_) => _MissingPickerDialog(
      songs: songs,
      plan: plan,
      warning: result.warning,
      downloadDirectory: c.appState?.downloadDirectory,
    ),
  );
  if (ids == null || ids.isEmpty || !context.mounted) return;
  await c.downloadMissingTracks(trackIds: ids);
}

/// 「一键同步」：说明会跳过 / 解密 / 下载各多少首，确认后开始。
Future<void> showSyncDialog(BuildContext context, AppController c) async {
  final result = await c.requestTransferPlan();
  if (!context.mounted) return;
  if (result == null) {
    final error = c.transferError;
    if (error != null) await showTransferMessage(context, '无法同步', error);
    return;
  }
  final error = result.error;
  if (error != null) {
    await showTransferMessage(context, '无法同步', error.message);
    return;
  }
  final plan = result.plan;
  if (plan == null) return;
  final missing = plan.missingRows.length;
  final overwrite = c.settings?.settings.overwriteExisting ?? false;
  final confirmed = await showDialog<bool>(
    context: context,
    builder: (ctx) => ContentDialog(
      title: const Text('一键同步'),
      content: SizedBox(
        width: 520,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text(
              '将处理 ${plan.total} 首：本地已有 ${plan.available} 首会跳过，'
              '${plan.ncm} 首是待解密的 NCM，$missing 首需要下载。',
            ),
            const SizedBox(height: 12),
            Text(
              '下载到：${c.appState?.downloadDirectory ?? '默认目录'}',
              style: const TextStyle(fontSize: 12),
            ),
            Text(
              overwrite ? '同名文件会被覆盖。' : '同名文件会跳过。',
              style: const TextStyle(fontSize: 12),
            ),
            if (result.warning != null) ...[
              const SizedBox(height: 8),
              Text(result.warning!),
            ],
          ],
        ),
      ),
      actions: [
        Button(onPressed: () => Navigator.pop(ctx, false), child: const Text('取消')),
        FilledButton(
          onPressed: () => Navigator.pop(ctx, true),
          child: const Text('开始同步'),
        ),
      ],
    ),
  );
  if (confirmed != true || !context.mounted) return;
  await c.syncCurrentPlaylist();
}

/// 缺失歌曲列表：勾选要下载的行，支持 Shift 连选。
class _MissingPickerDialog extends StatefulWidget {
  const _MissingPickerDialog({
    required this.songs,
    required this.plan,
    required this.warning,
    required this.downloadDirectory,
  });

  final List<SongDto> songs;
  final TransferPlanDto plan;
  final String? warning;
  final String? downloadDirectory;

  @override
  State<_MissingPickerDialog> createState() => _MissingPickerDialogState();
}

class _MissingPickerDialogState extends State<_MissingPickerDialog> {
  /// 选中的位置（plan.missingRows 的下标，不是行号）。
  late final Set<int> _selected = widget.plan.missingRows.toSet();
  int? _anchor;

  void _toggle(int position, {required bool control, required bool shift}) {
    setState(() {
      if (shift && _anchor != null) {
        final start = _anchor! < position ? _anchor! : position;
        final end = _anchor! < position ? position : _anchor!;
        if (!control) _selected.clear();
        _selected.addAll([for (var i = start; i <= end; i++) i]);
      } else if (control) {
        if (!_selected.remove(position)) _selected.add(position);
        _anchor = position;
      } else {
        _selected
          ..clear()
          ..add(position);
        _anchor = position;
      }
    });
  }

  List<int> _ids() {
    final positions = _selected.toList()..sort();
    final rows = widget.plan.missingRows;
    final out = <int>[];
    for (final position in positions) {
      if (position < rows.length) {
        final row = rows[position];
        if (row < widget.songs.length) out.add(widget.songs[row].id);
      }
    }
    return out;
  }

  @override
  Widget build(BuildContext context) {
    final res = FluentTheme.of(context).resources;
    final rows = widget.plan.missingRows;
    return ContentDialog(
      title: const Text('下载缺失的歌曲'),
      content: SizedBox(
        width: 580,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Text('共 ${rows.length} 首需要下载，已勾选 ${_selected.length} 首。'),
            const SizedBox(height: 6),
            Row(
              children: [
                HyperlinkButton(
                  onPressed: () => setState(() {
                    _selected
                      ..clear()
                      ..addAll([for (var i = 0; i < rows.length; i++) i]);
                    _anchor = null;
                  }),
                  child: const Text('全选'),
                ),
                const SizedBox(width: 8),
                HyperlinkButton(
                  onPressed: () => setState(() {
                    _selected.clear();
                    _anchor = null;
                  }),
                  child: const Text('全不选'),
                ),
                const Spacer(),
                Text(
                  widget.downloadDirectory ?? '默认目录',
                  style: TextStyle(color: res.textFillColorSecondary, fontSize: 12),
                ),
              ],
            ),
            const SizedBox(height: 6),
            Container(
              height: 280,
              decoration: BoxDecoration(
                border: Border.all(color: res.dividerStrokeColorDefault),
                borderRadius: BorderRadius.circular(6),
              ),
              child: ListView.builder(
                padding: const EdgeInsets.symmetric(vertical: 4),
                itemCount: rows.length,
                itemBuilder: (context, position) {
                  final row = rows[position];
                  final song = row < widget.songs.length ? widget.songs[row] : null;
                  return GestureDetector(
                    behavior: HitTestBehavior.opaque,
                    onTap: () {
                      final keyboard = HardwareKeyboard.instance;
                      _toggle(
                        position,
                        control: keyboard.isControlPressed,
                        shift: keyboard.isShiftPressed,
                      );
                    },
                    child: Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 2),
                      child: Row(
                        children: [
                          Checkbox(
                            checked: _selected.contains(position),
                            onChanged: (value) => setState(() {
                              if (value ?? false) {
                                _selected.add(position);
                              } else {
                                _selected.remove(position);
                              }
                              _anchor = position;
                            }),
                          ),
                          const SizedBox(width: 8),
                          Expanded(
                            child: Column(
                              crossAxisAlignment: CrossAxisAlignment.start,
                              mainAxisSize: MainAxisSize.min,
                              children: [
                                Text(
                                  song?.title ?? '第 ${row + 1} 行',
                                  maxLines: 1,
                                  overflow: TextOverflow.ellipsis,
                                ),
                                if (song != null)
                                  Text(
                                    song.artists,
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis,
                                    style: TextStyle(
                                      color: res.textFillColorSecondary,
                                      fontSize: 12,
                                    ),
                                  ),
                              ],
                            ),
                          ),
                        ],
                      ),
                    ),
                  );
                },
              ),
            ),
            if (widget.warning != null) ...[
              const SizedBox(height: 8),
              Text(
                widget.warning!,
                style: TextStyle(color: res.systemFillColorCaution),
              ),
            ],
          ],
        ),
      ),
      actions: [
        Button(onPressed: () => Navigator.pop(context), child: const Text('取消')),
        FilledButton(
          onPressed: _selected.isEmpty
              ? null
              : () => Navigator.pop(context, _ids()),
          child: Text('下载 ${_selected.length} 首'),
        ),
      ],
    );
  }
}
