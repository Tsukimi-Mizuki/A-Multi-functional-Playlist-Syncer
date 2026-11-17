import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter/services.dart';
import 'package:lunatic_princess/controller.dart';
import 'package:lunatic_princess/format.dart';
import 'package:lunatic_princess/src/rust/api/library.dart';
import 'package:lunatic_princess/src/rust/api/playlists.dart';

const double _rowHeight = 40;
const double _headerHeight = 36;
const double _statusWidth = 24;

class PlaylistPage extends StatelessWidget {
  const PlaylistPage({super.key, required this.controller});

  final AppController controller;

  int? get _id => int.tryParse(controller.selectedKey.substring(3));

  @override
  Widget build(BuildContext context) {
    final c = controller;
    final d = c.detail;
    final id = _id;

    Widget body;
    if (c.detailError != null) {
      body = Padding(
        padding: const EdgeInsets.fromLTRB(24, 0, 24, 16),
        child: InfoBar(
          title: const Text('加载歌单失败'),
          content: Text(c.detailError!),
          severity: InfoBarSeverity.error,
          isLong: true,
          action: id == null
              ? null
              : Button(
                  onPressed: () => c.loadDetail(id),
                  child: const Text('重试'),
                ),
        ),
      );
    } else if (c.detailLoading || d == null) {
      body = const Center(child: ProgressRing());
    } else {
      body = Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (d.isPartial)
            Padding(
              padding: const EdgeInsets.fromLTRB(24, 0, 24, 8),
              child: InfoBar(
                title: const Text('歌曲列表不完整'),
                content: const Text('部分歌曲暂时没有取到，可以重新加载。'),
                severity: InfoBarSeverity.warning,
                isLong: true,
                action: Button(
                  onPressed: () => c.loadDetail(d.id),
                  child: const Text('重新加载'),
                ),
              ),
            ),
          if (c.matchError != null)
            Padding(
              padding: const EdgeInsets.fromLTRB(24, 0, 24, 8),
              child: InfoBar(
                title: const Text('对比本地曲库失败'),
                content: Text(c.matchError!),
                severity: InfoBarSeverity.error,
                isLong: true,
                action: Button(
                  onPressed: () => c.matchLocal(),
                  child: const Text('重试'),
                ),
              ),
            ),
          if (c.notice != null)
            Padding(
              padding: const EdgeInsets.fromLTRB(24, 0, 24, 8),
              child: InfoBar(
                title: Text(c.notice!),
                severity: InfoBarSeverity.info,
                isLong: true,
                action: Button(
                  onPressed: c.clearNotice,
                  child: const Text('知道了'),
                ),
              ),
            ),
          Expanded(
            child: d.songs.isEmpty
                ? const Center(child: Text('这个歌单还没有歌曲'))
                : _SongTable(
                    songs: d.songs,
                    matches: c.match?.matches,
                  ),
          ),
        ],
      );
    }

    return ScaffoldPage(
      padding: EdgeInsets.zero,
      header: _PlaylistHeader(controller: c),
      content: body,
    );
  }
}

class _PlaylistHeader extends StatelessWidget {
  const _PlaylistHeader({required this.controller});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final c = controller;
    final d = c.detail;
    final res = FluentTheme.of(context).resources;
    final theme = FluentTheme.of(context);

    final name = d?.name ?? _titleFallback(c);
    final subtitle = _subtitle(c, d);

    return Padding(
      padding: const EdgeInsets.fromLTRB(24, 12, 24, 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: MainAxisSize.min,
        children: [
          Row(
            crossAxisAlignment: CrossAxisAlignment.center,
            children: [
              _Cover(url: d?.coverUrl),
              const SizedBox(width: 16),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  mainAxisSize: MainAxisSize.min,
                  children: [
                    Text(
                      name,
                      style: theme.typography.subtitle,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                    ),
                    if (subtitle.isNotEmpty) ...[
                      const SizedBox(height: 2),
                      Text(
                        subtitle,
                        style: TextStyle(color: res.textFillColorSecondary),
                      ),
                    ],
                  ],
                ),
              ),
              const SizedBox(width: 12),
              _HeaderActions(controller: c),
            ],
          ),
          if (c.libraryBusy)
            Padding(
              padding: const EdgeInsets.only(top: 10),
              child: _ScanProgress(progress: c.scanProgress),
            ),
        ],
      ),
    );
  }

  /// 副标题：创建者、曲目数；对比过后再加本地统计。
  String _subtitle(AppController c, PlaylistDetailDto? d) {
    if (d == null) return '';
    final parts = <String>[
      if (d.creatorNickname != null) '创建者：${d.creatorNickname}',
      '${d.trackCount} 首',
    ];
    final match = c.match;
    if (match != null && match.scanned) {
      final stats = match.stats;
      parts.add('本地 ${stats.available}');
      if (stats.ncm > 0) parts.add('NCM ${stats.ncm}');
      parts.add('缺失 ${stats.missing}');
    }
    return parts.join('  ·  ');
  }

  String _titleFallback(AppController c) {
    for (final p in c.playlists) {
      if (playlistKey(p.id) == c.selectedKey) return p.name;
    }
    return '歌单';
  }
}

/// 歌单页右上角的操作：对比本地曲库（主）、重新扫描、刷新。
class _HeaderActions extends StatelessWidget {
  const _HeaderActions({required this.controller});

  final AppController controller;

  @override
  Widget build(BuildContext context) {
    final c = controller;
    final d = c.detail;
    final busy = c.libraryBusy;
    return Row(
      mainAxisSize: MainAxisSize.min,
      children: [
        if (busy)
          Button(
            onPressed: c.cancelLibrary,
            child: const Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                WindowsIcon(WindowsIcons.cancel, size: 14),
                SizedBox(width: 6),
                Text('取消'),
              ],
            ),
          )
        else
          FilledButton(
            onPressed: d == null ? null : () => c.matchLocal(),
            child: const Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                WindowsIcon(WindowsIcons.search, size: 14),
                SizedBox(width: 6),
                Text('对比本地曲库'),
              ],
            ),
          ),
        const SizedBox(width: 8),
        Button(
          onPressed: d == null || busy ? null : () => c.matchLocal(rescan: true),
          child: const Text('重新扫描'),
        ),
        const SizedBox(width: 8),
        Button(
          onPressed: d == null ? null : () => c.loadDetail(d.id),
          child: const Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              WindowsIcon(WindowsIcons.refresh, size: 14),
              SizedBox(width: 6),
              Text('刷新'),
            ],
          ),
        ),
      ],
    );
  }
}

/// 扫描 / 对比进度条。
class _ScanProgress extends StatelessWidget {
  const _ScanProgress({required this.progress});

  final ScanProgressDto? progress;

  @override
  Widget build(BuildContext context) {
    final res = FluentTheme.of(context).resources;
    final p = progress;
    final total = p?.total ?? 0;
    final scanned = p?.scanned ?? 0;
    final text = p == null
        ? '正在准备…'
        : (total <= 0
            ? '正在统计文件…'
            : '$scanned / $total${p.fileName.isEmpty ? '' : '  ·  ${p.fileName}'}');
    return Row(
      children: [
        Expanded(
          child: ProgressBar(
            value: total > 0 ? (scanned / total) * 100 : null,
            strokeWidth: 3,
          ),
        ),
        const SizedBox(width: 12),
        SizedBox(
          width: 320,
          child: Text(
            text,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            textAlign: TextAlign.right,
            style: TextStyle(color: res.textFillColorSecondary, fontSize: 12),
          ),
        ),
      ],
    );
  }
}

/// 歌单封面；网络拉不到时退回一个图标占位，不让布局跳动。
class _Cover extends StatelessWidget {
  const _Cover({required this.url});

  final String? url;

  @override
  Widget build(BuildContext context) {
    final res = FluentTheme.of(context).resources;
    final placeholder = Container(
      width: 72,
      height: 72,
      decoration: BoxDecoration(
        color: res.subtleFillColorSecondary,
        borderRadius: BorderRadius.circular(8),
      ),
      child: WindowsIcon(
        WindowsIcons.music_note,
        size: 28,
        color: res.textFillColorSecondary,
      ),
    );
    final src = url;
    if (src == null || src.isEmpty) return placeholder;
    return ClipRRect(
      borderRadius: BorderRadius.circular(8),
      child: Image.network(
        src,
        width: 72,
        height: 72,
        fit: BoxFit.cover,
        errorBuilder: (_, _, _) => placeholder,
        loadingBuilder: (context, child, progress) =>
            progress == null ? child : placeholder,
      ),
    );
  }
}

/// 歌曲表格：表头 + 可选中（Ctrl 多选、Shift 连选）的行。
class _SongTable extends StatefulWidget {
  const _SongTable({required this.songs, required this.matches});

  final List<SongDto> songs;

  /// 与 [songs] 按顺序一一对应的本地匹配结果；还没对比时为 null。
  final List<LocalMatchDto>? matches;

  @override
  State<_SongTable> createState() => _SongTableState();
}

class _SongTableState extends State<_SongTable> {
  /// 选中的行下标（同一首歌可能在歌单里出现两次，所以按行算）。
  final Set<int> _selected = <int>{};
  int? _anchor;

  @override
  void didUpdateWidget(covariant _SongTable oldWidget) {
    super.didUpdateWidget(oldWidget);
    // 换了歌单或曲目数变了，旧的行号就没有意义了。
    if (oldWidget.songs.length != widget.songs.length ||
        (widget.songs.isNotEmpty &&
            oldWidget.songs.isNotEmpty &&
            oldWidget.songs.first.id != widget.songs.first.id)) {
      _selected.clear();
      _anchor = null;
    }
  }

  void _onTap(int index, {required bool control, required bool shift}) {
    setState(() {
      if (shift && _anchor != null) {
        final start = _anchor! < index ? _anchor! : index;
        final end = _anchor! < index ? index : _anchor!;
        if (!control) _selected.clear();
        _selected.addAll([for (var i = start; i <= end; i++) i]);
      } else if (control) {
        if (!_selected.remove(index)) _selected.add(index);
        _anchor = index;
      } else {
        _selected
          ..clear()
          ..add(index);
        _anchor = index;
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final res = FluentTheme.of(context).resources;
    final selectedCount = _selected.length;
    return Column(
      children: [
        SizedBox(
          height: _headerHeight,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 24),
            child: DefaultTextStyle.merge(
              style: TextStyle(
                color: res.textFillColorSecondary,
                fontWeight: FontWeight.w600,
                fontSize: 12,
              ),
              child: Row(
                children: [
                  const SizedBox(width: _statusWidth),
                  Expanded(
                    child: _Columns(
                      index: const Text('#'),
                      title: const Text('标题'),
                      artist: const Text('歌手'),
                      album: const Text('专辑'),
                      duration: const Padding(
                        padding: EdgeInsets.only(right: 4),
                        child: Text('时长'),
                      ),
                    ),
                  ),
                  if (selectedCount > 0) ...[
                    const SizedBox(width: 12),
                    Text('已选 $selectedCount 首'),
                    const SizedBox(width: 8),
                    HyperlinkButton(
                      onPressed: () => setState(_selected.clear),
                      child: const Text('清除'),
                    ),
                  ],
                ],
              ),
            ),
          ),
        ),
        Divider(style: DividerThemeData(horizontalMargin: EdgeInsets.zero)),
        Expanded(
          child: ListView.builder(
            itemExtent: _rowHeight,
            itemCount: widget.songs.length,
            itemBuilder: (context, i) => _SongRow(
              index: i,
              song: widget.songs[i],
              match: widget.matches != null && i < widget.matches!.length
                  ? widget.matches![i]
                  : null,
              selected: _selected.contains(i),
              onTap: (control, shift) => _onTap(i, control: control, shift: shift),
            ),
          ),
        ),
      ],
    );
  }
}

/// 表头与行共用的列布局，保证对齐。
class _Columns extends StatelessWidget {
  const _Columns({
    required this.index,
    required this.title,
    required this.artist,
    required this.album,
    required this.duration,
  });

  final Widget index;
  final Widget title;
  final Widget artist;
  final Widget album;
  final Widget duration;

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        SizedBox(width: 48, child: index),
        Expanded(flex: 5, child: title),
        Expanded(flex: 3, child: artist),
        Expanded(flex: 3, child: album),
        SizedBox(
          width: 72,
          child: Align(alignment: Alignment.centerRight, child: duration),
        ),
      ],
    );
  }
}

class _SongRow extends StatefulWidget {
  const _SongRow({
    required this.index,
    required this.song,
    required this.match,
    required this.selected,
    required this.onTap,
  });

  final int index;
  final SongDto song;
  final LocalMatchDto? match;
  final bool selected;
  final void Function(bool control, bool shift) onTap;

  @override
  State<_SongRow> createState() => _SongRowState();
}

class _SongRowState extends State<_SongRow> {
  bool _hover = false;

  @override
  Widget build(BuildContext context) {
    final res = FluentTheme.of(context).resources;
    final s = widget.song;
    const ellipsis = TextOverflow.ellipsis;
    final selected = widget.selected;
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () {
          final keyboard = HardwareKeyboard.instance;
          widget.onTap(keyboard.isControlPressed, keyboard.isShiftPressed);
        },
        child: Container(
          margin: const EdgeInsets.symmetric(horizontal: 12),
          padding: const EdgeInsets.symmetric(horizontal: 12),
          decoration: BoxDecoration(
            color: selected
                ? res.subtleFillColorTertiary
                : (_hover ? res.subtleFillColorSecondary : Colors.transparent),
            borderRadius: BorderRadius.circular(4),
          ),
          child: Row(
            children: [
              SizedBox(
                width: _statusWidth,
                child: _AvailabilityDot(match: widget.match),
              ),
              Expanded(
                child: _Columns(
                  index: Text(
                    '${widget.index + 1}',
                    style: TextStyle(color: res.textFillColorSecondary),
                  ),
                  title: Text.rich(
                    TextSpan(
                      text: s.title,
                      children: [
                        if (s.alias != null && s.alias!.isNotEmpty)
                          TextSpan(
                            text: '  ${s.alias}',
                            style: TextStyle(color: res.textFillColorSecondary),
                          ),
                      ],
                    ),
                    maxLines: 1,
                    overflow: ellipsis,
                  ),
                  artist: Text(s.artists, maxLines: 1, overflow: ellipsis),
                  album: Text(s.album, maxLines: 1, overflow: ellipsis),
                  duration: Text(
                    formatDuration(s.durationMs),
                    style: TextStyle(color: res.textFillColorSecondary),
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// 本地状态的圆点：绿=本地已有，橙=NCM 待解密，空心=缺失（未对比时不显示）。
class _AvailabilityDot extends StatelessWidget {
  const _AvailabilityDot({required this.match});

  final LocalMatchDto? match;

  static const _available = Color(0xFF16A34A);
  static const _ncm = Color(0xFFD97706);

  @override
  Widget build(BuildContext context) {
    final res = FluentTheme.of(context).resources;
    final m = match;
    if (m == null) return const SizedBox.shrink();
    final (color, label) = switch (m.availability) {
      'available' => (_available, '本地已有'),
      'ncm' => (_ncm, 'NCM 待解密'),
      _ => (null, '缺失（需要下载）'),
    };
    final dot = Container(
      width: 8,
      height: 8,
      decoration: BoxDecoration(
        color: color,
        shape: BoxShape.circle,
        border: color == null ? Border.all(color: res.textFillColorSecondary, width: 1) : null,
      ),
    );
    final detail = [
      label,
      if (m.fileName != null) m.fileName!,
      if (m.method != 'none') _methodLabel(m.method),
      if (m.confidence > 0) '置信度 ${(m.confidence * 100).round()}%',
    ].join('  ·  ');
    return Tooltip(
      message: detail,
      child: Align(alignment: Alignment.centerLeft, child: dot),
    );
  }

  String _methodLabel(String method) => switch (method) {
        'neteaseId' => '网易云 ID 匹配',
        'metadata' => '元数据匹配',
        'fileName' => '文件名匹配',
        _ => '未匹配',
      };
}
