import 'dart:async';
import 'dart:math' as math;

import 'package:fluent_ui/fluent_ui.dart';
import 'package:lunatic_princess/src/rust/api/online.dart';

/// 在线标签取回的结果：编辑器按它回填表单（只改标签字段与封面，歌词不动）。
class OnlineTagsResult {
  const OnlineTagsResult({
    required this.loaded,
    required this.tagsRequested,
    required this.coverRequested,
  });

  final LoadedTagsDto loaded;

  /// 用户勾了「标签」。
  final bool tagsRequested;

  /// 用户勾了「封面」。
  final bool coverRequested;
}

/// 打开「在线标签」面板：同时搜网易云与 QQ 音乐，选一条候选取标签与封面。
///
/// 返回 `null` 表示取消（点遮罩、Esc 或「取消」）；确定后由调用方负责把内容填进编辑器，
/// 本面板不写文件、也不改歌词。
Future<OnlineTagsResult?> showOnlineTagsDialog(
  BuildContext context, {
  required String query,
  FileContextDto? fileContext,
}) {
  return showDialog<OnlineTagsResult>(
    context: context,
    barrierDismissible: true,
    builder: (context) =>
        _OnlineTagsDialog(query: query, fileContext: fileContext),
  );
}

const double _coverWidth = 104;
const double _sourceWidth = 118;
const double _timeWidth = 76;
const double _rowHeight = 120;

class _OnlineTagsDialog extends StatefulWidget {
  const _OnlineTagsDialog({required this.query, this.fileContext});

  final String query;
  final FileContextDto? fileContext;

  @override
  State<_OnlineTagsDialog> createState() => _OnlineTagsDialogState();
}

class _OnlineTagsDialogState extends State<_OnlineTagsDialog> {
  late final TextEditingController _query = TextEditingController(
    text: widget.query,
  );

  bool _searching = false;
  bool _confirming = false;
  String _status = '';
  String _failure = '';
  List<TagCandidateDto> _items = const [];
  int? _selected;
  bool _wantTags = true;
  bool _wantCover = true;

  /// 封面尺寸：`来源:ID` → 「读取尺寸…」「600×600」「尺寸未知」「无封面」。
  final Map<String, String> _coverSizes = {};

  /// 每次搜索加一，旧的封面尺寸探测结果回来时直接丢掉。
  int _generation = 0;

  @override
  void initState() {
    super.initState();
    _search();
  }

  @override
  void dispose() {
    _query.dispose();
    super.dispose();
  }

  String _key(OnlineSongDto song) => '${song.source}:${song.sourceId}';

  Future<void> _search() async {
    if (_searching || _confirming) return;
    final query = _query.text.trim();
    if (query.isEmpty && widget.fileContext?.filePath == null) {
      setState(() => _status = '请输入要搜索的歌曲、歌手或专辑。');
      return;
    }
    final generation = ++_generation;
    setState(() {
      _searching = true;
      _status = '正在搜索网易云音乐与 QQ 音乐...';
      _failure = '';
      _items = const [];
      _selected = null;
      _coverSizes.clear();
    });
    try {
      final result = await onlineTagCandidates(
        query: query,
        fileContext: widget.fileContext,
      );
      if (!mounted || generation != _generation) return;
      final error = result.error;
      if (error != null) {
        setState(() {
          _searching = false;
          _status = '搜索失败：${error.message}';
        });
        return;
      }
      final data = result.data;
      if (data == null) {
        setState(() {
          _searching = false;
          _status = '搜索失败：请稍后重试';
        });
        return;
      }
      final items = List<TagCandidateDto>.of(data.items);
      final counts = <String, int>{};
      for (final item in items) {
        counts.update(item.song.sourceLabel, (value) => value + 1,
            ifAbsent: () => 1);
      }
      final buffer = StringBuffer();
      if (items.isEmpty) {
        buffer.write('没有找到候选，换个关键词试试。');
      } else {
        buffer.write('找到 ${items.length} 条候选');
        if (counts.isNotEmpty) {
          buffer.write('（${counts.entries.map((e) => '${e.key} ${e.value}').join('、')}）');
        }
        if (data.scored) {
          buffer.write(
            '，按匹配度从高到低排列（低于 ${data.thresholdPercent}% 的已标注）',
          );
        }
        buffer.write('。');
      }
      setState(() {
        _searching = false;
        _items = items;
        _selected = items.isEmpty ? null : 0;
        _status = buffer.toString();
        _failure = data.failures.isEmpty
            ? ''
            : ' 部分来源没有搜到：${data.failures.join('；')}';
      });
      unawaited(_probeCoverSizes(items, generation));
    } catch (error) {
      if (!mounted || generation != _generation) return;
      setState(() {
        _searching = false;
        _status = '搜索失败：$error';
      });
    }
  }

  /// 只处理有封面地址的行，最多 4 个并发；结果回来时搜索已经换了就直接丢掉。
  Future<void> _probeCoverSizes(
    List<TagCandidateDto> items,
    int generation,
  ) async {
    final pending = <TagCandidateDto>[];
    for (final item in items) {
      if (item.thumbnailUrl == null) {
        _coverSizes[_key(item.song)] = '无封面';
      } else {
        pending.add(item);
      }
    }
    if (!mounted) return;
    setState(() {
      for (final item in pending) {
        _coverSizes[_key(item.song)] = '读取尺寸…';
      }
    });
    var next = 0;
    Future<void> worker() async {
      while (next < pending.length) {
        if (!mounted || generation != _generation) return;
        final item = pending[next++];
        String text;
        try {
          final result = await onlineCoverSize(song: item.song);
          final size = result.size;
          text = size == null ? '尺寸未知' : '${size.width}×${size.height}';
        } catch (_) {
          text = '尺寸未知';
        }
        if (!mounted || generation != _generation) return;
        setState(() => _coverSizes[_key(item.song)] = text);
      }
    }

    await Future.wait([for (var i = 0; i < 4; i++) worker()]);
  }

  Future<void> _confirm() async {
    if (_confirming || _searching) return;
    final index = _selected;
    if (index == null) {
      setState(() => _status = '请先选择一条候选。');
      return;
    }
    if (!_wantTags && !_wantCover) {
      setState(() => _status = '请至少勾选「标签」或「封面」中的一项。');
      return;
    }
    final item = _items[index];
    final label = item.song.sourceLabel;
    final what = _wantTags && _wantCover
        ? '标签与封面'
        : (_wantTags ? '标签' : '封面');
    setState(() {
      _confirming = true;
      _status = '正在获取$label的$what...';
    });
    try {
      final result = await onlineLoadTags(song: item.song, cover: _wantCover);
      if (!mounted) return;
      final error = result.error;
      if (error != null) {
        setState(() {
          _confirming = false;
          _status = '获取失败：${error.message}';
        });
        return;
      }
      final data = result.data;
      if (data == null) {
        setState(() {
          _confirming = false;
          _status = '获取失败：没有返回内容，请重试。';
        });
        return;
      }
      if (!_wantTags && data.cover == null) {
        setState(() {
          _confirming = false;
          _status = '没有获取到$label的封面，可以换一条候选。';
        });
        return;
      }
      Navigator.of(context).pop(
        OnlineTagsResult(
          loaded: data,
          tagsRequested: _wantTags,
          coverRequested: _wantCover,
        ),
      );
    } catch (error) {
      if (!mounted) return;
      setState(() {
        _confirming = false;
        _status = '获取失败：$error';
      });
    }
  }

  @override
  Widget build(BuildContext context) {
    final theme = FluentTheme.of(context);
    final screen = MediaQuery.sizeOf(context);
    final width = math.min(1120.0, screen.width - 64);
    final height = math.min(820.0, screen.height - 64);
    return Center(
      child: SizedBox(
        width: width,
        height: height,
        child: Card(
          padding: EdgeInsets.zero,
          child: Column(
            children: [
              _buildHeader(theme),
              Divider(style: DividerThemeData(horizontalMargin: EdgeInsets.zero)),
              _buildSearchRow(),
              _buildStatus(theme),
              Divider(style: DividerThemeData(horizontalMargin: EdgeInsets.zero)),
              Expanded(child: _buildList(theme)),
              Divider(style: DividerThemeData(horizontalMargin: EdgeInsets.zero)),
              _buildFooter(theme),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildHeader(FluentThemeData theme) {
    final res = theme.resources;
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 12, 8, 12),
      child: Row(
        children: [
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text('在线标签', style: theme.typography.subtitle),
                const SizedBox(height: 2),
                Text(
                  '按关键词同时搜网易云音乐与 QQ 音乐，取标签字段与封面；歌词不会改动。',
                  style: TextStyle(
                    fontSize: 12,
                    color: res.textFillColorSecondary,
                  ),
                ),
              ],
            ),
          ),
          IconButton(
            icon: const Icon(WindowsIcons.cancel, size: 14),
            onPressed: () => Navigator.of(context).pop(),
          ),
        ],
      ),
    );
  }

  Widget _buildSearchRow() {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 12, 20, 4),
      child: Row(
        children: [
          Expanded(
            child: TextBox(
              controller: _query,
              placeholder: '歌曲、歌手或专辑',
              onSubmitted: (_) => _search(),
            ),
          ),
          const SizedBox(width: 10),
          FilledButton(
            onPressed: _searching || _confirming ? null : _search,
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (_searching)
                  const Padding(
                    padding: EdgeInsets.only(right: 8),
                    child: SizedBox(
                      width: 14,
                      height: 14,
                      child: ProgressRing(),
                    ),
                  ),
                const Text('搜索'),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildStatus(FluentThemeData theme) {
    final res = theme.resources;
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 0, 20, 10),
      child: SizedBox(
        height: 18,
        child: Row(
          children: [
            Flexible(
              child: Text(
                _status,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  fontSize: 12,
                  color: res.textFillColorSecondary,
                ),
              ),
            ),
            if (_failure.isNotEmpty)
              Flexible(
                child: Text(
                  _failure,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 12,
                    color: res.systemFillColorCaution,
                  ),
                ),
              ),
          ],
        ),
      ),
    );
  }

  Widget _buildList(FluentThemeData theme) {
    final res = theme.resources;
    return Column(
      children: [
        SizedBox(height: 30, child: _buildHeaderRow(theme)),
        Divider(style: DividerThemeData(horizontalMargin: EdgeInsets.zero)),
        Expanded(
          child: _items.isEmpty
              ? Center(
                  child: Text(
                    _searching ? '正在搜索…' : '没有候选。改一下上面的关键词再搜索。',
                    style: TextStyle(color: res.textFillColorSecondary),
                  ),
                )
              : ListView.builder(
                  itemExtent: _rowHeight,
                  itemCount: _items.length,
                  itemBuilder: (context, index) => _buildRow(theme, index),
                ),
        ),
      ],
    );
  }

  Widget _buildHeaderRow(FluentThemeData theme) {
    final res = theme.resources;
    final style = TextStyle(
      fontSize: 12,
      fontWeight: FontWeight.w600,
      color: res.textFillColorSecondary,
    );
    Widget cell(String label, {int flex = 1, double? width, TextAlign? align}) {
      final text = Text(label, style: style, textAlign: align);
      if (width != null) {
        return SizedBox(width: width, child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 10),
          child: Align(alignment: Alignment.centerLeft, child: text),
        ));
      }
      return Expanded(
        flex: flex,
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 10),
          child: Align(alignment: Alignment.centerLeft, child: text),
        ),
      );
    }

    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: 12),
      child: Row(
        children: [
          cell('封面', width: _coverWidth),
          cell('源', width: _sourceWidth),
          cell('标题', flex: 15),
          cell('艺术家', flex: 10),
          cell('专辑', flex: 13),
          cell('时长 / 匹配', width: _timeWidth, align: TextAlign.center),
        ],
      ),
    );
  }

  Widget _buildRow(FluentThemeData theme, int index) {
    final res = theme.resources;
    final accent = theme.accentColor;
    final item = _items[index];
    final song = item.song;
    final selected = index == _selected;
    final score = item.score;

    Widget padded(Widget child, {int flex = 1, double? width}) {
      final content = Padding(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        child: child,
      );
      if (width != null) return SizedBox(width: width, child: content);
      return Expanded(flex: flex, child: content);
    }

    Widget lines(List<Widget> children) => Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisAlignment: MainAxisAlignment.center,
      children: children,
    );

    final facts = _facts(item).join('  ·  ');
    final coverText = _coverSizes[_key(song)];

    return MouseRegion(
      cursor: SystemMouseCursors.click,
      child: GestureDetector(
        behavior: HitTestBehavior.opaque,
        onTap: () => setState(() => _selected = index),
        onDoubleTap: () {
          setState(() => _selected = index);
          unawaited(_confirm());
        },
        child: Container(
          decoration: BoxDecoration(
            color: selected ? res.subtleFillColorSecondary : null,
            border: Border(
              left: BorderSide(
                color: selected ? accent : const Color(0x00000000),
                width: 3,
              ),
              bottom: BorderSide(
                color: res.dividerStrokeColorDefault,
                width: 0.5,
              ),
            ),
          ),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 9),
            child: Row(
              children: [
                padded(
                  _thumbnail(theme, item),
                  width: _coverWidth,
                ),
                padded(
                  lines([
                    Text(
                      song.sourceLabel,
                      maxLines: 1,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(
                        fontSize: 13,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                    if (coverText != null) ...[
                      const SizedBox(height: 2),
                      Text(
                        coverText,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontSize: 11,
                          color: res.textFillColorSecondary,
                        ),
                      ),
                    ],
                    if (facts.isNotEmpty) ...[
                      const SizedBox(height: 2),
                      Text(
                        facts,
                        maxLines: 2,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontSize: 11,
                          color: res.textFillColorSecondary,
                        ),
                      ),
                    ],
                  ]),
                  width: _sourceWidth,
                ),
                padded(
                  lines([
                    Text(
                      song.displayTitle,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(
                        fontSize: 14,
                        fontWeight: FontWeight.w600,
                      ),
                    ),
                  ]),
                  flex: 15,
                ),
                padded(
                  lines([
                    Text(
                      song.artist.isEmpty ? '未知歌手' : song.artist,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: const TextStyle(fontSize: 13),
                    ),
                  ]),
                  flex: 10,
                ),
                padded(
                  lines([
                    Text(
                      song.album.isEmpty ? '未知专辑' : song.album,
                      maxLines: 2,
                      overflow: TextOverflow.ellipsis,
                      style: TextStyle(
                        fontSize: 13,
                        color: res.textFillColorSecondary,
                      ),
                    ),
                  ]),
                  flex: 13,
                ),
                padded(
                  lines([
                    Text(
                      song.durationText,
                      style: TextStyle(
                        fontSize: 13,
                        fontFeatures: const [FontFeature.tabularFigures()],
                        color: res.textFillColorSecondary,
                      ),
                    ),
                    if (score != null) ...[
                      const SizedBox(height: 2),
                      Text(
                        '匹配 ${score.percent}%',
                        style: TextStyle(
                          fontSize: 11,
                          color: item.belowThreshold
                              ? res.systemFillColorCaution
                              : res.textFillColorSecondary,
                        ),
                      ),
                    ],
                  ]),
                  width: _timeWidth,
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  List<String> _facts(TagCandidateDto item) {
    final song = item.song;
    return [
      if (song.year != null) '${song.year}',
      if (song.trackNumber != null) '音轨 ${song.trackNumber}',
      if ((song.discNumber ?? 0) > 1) '碟 ${song.discNumber}',
      if ((song.genre ?? '').isNotEmpty) song.genre!,
    ];
  }

  Widget _thumbnail(FluentThemeData theme, TagCandidateDto item) {
    final res = theme.resources;
    final placeholder = Container(
      width: _coverWidth,
      height: _coverWidth,
      decoration: BoxDecoration(
        color: res.subtleFillColorSecondary,
        borderRadius: BorderRadius.circular(8),
      ),
      child: Center(
        child: Text(
          '♪',
          style: TextStyle(fontSize: 28, color: res.textFillColorSecondary),
        ),
      ),
    );
    final url = item.thumbnailUrl;
    if (url == null) return placeholder;
    return ClipRRect(
      borderRadius: BorderRadius.circular(8),
      child: Image.network(
        url,
        width: _coverWidth,
        height: _coverWidth,
        fit: BoxFit.cover,
        gaplessPlayback: true,
        errorBuilder: (_, _, _) => placeholder,
        loadingBuilder: (context, child, progress) =>
            progress == null ? child : placeholder,
      ),
    );
  }

  Widget _buildFooter(FluentThemeData theme) {
    final res = theme.resources;
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 10, 20, 10),
      child: Row(
        children: [
          Text('要套用的内容', style: TextStyle(fontSize: 13)),
          const SizedBox(width: 12),
          Checkbox(
            checked: _wantTags,
            onChanged: _confirming
                ? null
                : (value) => setState(() => _wantTags = value ?? false),
            content: const Text('标签'),
          ),
          const SizedBox(width: 10),
          Checkbox(
            checked: _wantCover,
            onChanged: _confirming
                ? null
                : (value) => setState(() => _wantCover = value ?? false),
            content: const Text('封面'),
          ),
          const SizedBox(width: 16),
          Expanded(
            child: Text(
              '歌词不会改动；确定后填进编辑器，保存才写入文件。',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(
                fontSize: 12,
                color: res.textFillColorSecondary,
              ),
            ),
          ),
          Button(
            onPressed: _confirming
                ? null
                : () => Navigator.of(context).pop(),
            child: const Text('取消'),
          ),
          const SizedBox(width: 10),
          FilledButton(
            onPressed: _confirming ? null : _confirm,
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (_confirming)
                  const Padding(
                    padding: EdgeInsets.only(right: 8),
                    child: SizedBox(
                      width: 14,
                      height: 14,
                      child: ProgressRing(),
                    ),
                  ),
                Text(_confirming ? '获取中…' : '确定'),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
