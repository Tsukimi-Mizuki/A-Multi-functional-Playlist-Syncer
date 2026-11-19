import 'dart:async';

import 'package:fluent_ui/fluent_ui.dart';
import 'package:lunatic_princess/src/rust/api/lyrics.dart';
import 'package:lunatic_princess/src/rust/api/playlists.dart';

/// 歌词预览：只读地看一首歌的歌词（逐行 / 逐字优先），不写任何文件。
Future<void> showLyricsDialog(BuildContext context, SongDto song) {
  return showDialog<void>(
    context: context,
    barrierDismissible: true,
    builder: (context) => _LyricsDialog(song: song),
  );
}

class _LyricsDialog extends StatefulWidget {
  const _LyricsDialog({required this.song});

  final SongDto song;

  @override
  State<_LyricsDialog> createState() => _LyricsDialogState();
}

class _LyricsDialogState extends State<_LyricsDialog> {
  static const String _lineMode = 'line';
  static const String _wordMode = 'word';

  /// 两种形式各缓存一份：切回去不重新联网。
  final Map<String, LyricsPreviewDto> _cache = <String, LyricsPreviewDto>{};
  String _mode = _lineMode;
  bool _loading = false;
  String? _error;
  int _seq = 0;

  @override
  void initState() {
    super.initState();
    unawaited(_fetch(_lineMode));
  }

  Future<void> _fetch(String mode, {bool force = false}) async {
    if (!force && _cache.containsKey(mode)) {
      setState(() {
        _mode = mode;
        _loading = false;
        _error = null;
      });
      return;
    }
    final song = widget.song;
    final seq = ++_seq;
    setState(() {
      _mode = mode;
      _loading = true;
      _error = null;
    });
    try {
      final result = await getSongLyrics(
        id: song.id,
        mode: mode,
        title: song.title,
        artist: song.artists,
        album: song.album,
        durationMs: song.durationMs,
      );
      if (!mounted || seq != _seq) return;
      final failure = result.error;
      final data = result.data;
      setState(() {
        _loading = false;
        if (failure != null) {
          _error = failure.message;
        } else if (data == null) {
          _error = '没有获取到歌词内容。';
        } else {
          _cache[mode] = data;
        }
      });
    } catch (error) {
      if (!mounted || seq != _seq) return;
      setState(() {
        _loading = false;
        _error = '获取歌词失败：$error';
      });
    }
  }

  String get _songTitle {
    final title = widget.song.title.trim();
    return title.isEmpty ? '未知歌曲' : title;
  }

  String get _artistText {
    final artist = widget.song.artists.trim();
    return artist.isEmpty ? '未知歌手' : artist;
  }

  String _metaText(LyricsPreviewDto? preview) {
    final parts = <String>[
      '来源：${preview == null || preview.source.isEmpty ? '按设置查找中' : preview.source}',
      if (preview != null) preview.isWordLevel ? '逐字歌词' : '逐行歌词',
      if (preview != null) preview.hasTranslation ? '含翻译' : '无翻译',
      if (preview != null) '${preview.lineCount} 行',
    ];
    return parts.join('  ·  ');
  }

  @override
  Widget build(BuildContext context) {
    final theme = FluentTheme.of(context);
    final size = MediaQuery.sizeOf(context);
    final width = (size.width - 48).clamp(560.0, 960.0);
    final height = (size.height - 56).clamp(420.0, 760.0);
    final preview = _cache[_mode];
    return Center(
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
              _buildToolbar(theme, preview),
              Expanded(child: _buildBody(theme, preview)),
              const Divider(style: DividerThemeData(horizontalMargin: EdgeInsets.zero)),
              _buildFooter(theme),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildHeader(FluentThemeData theme) {
    final resources = theme.resources;
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 14, 12, 12),
      child: Row(
        children: <Widget>[
          Text('歌词预览', style: theme.typography.subtitle),
          const SizedBox(width: 12),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: <Widget>[
                Text(
                  _songTitle,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w600),
                ),
                Text(
                  _artistText,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 12,
                    color: resources.textFillColorSecondary,
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(width: 12),
          IconButton(
            icon: const WindowsIcon(WindowsIcons.cancel, size: 12),
            onPressed: () => Navigator.of(context).pop(),
          ),
        ],
      ),
    );
  }

  Widget _buildToolbar(FluentThemeData theme, LyricsPreviewDto? preview) {
    final resources = theme.resources;
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 0, 20, 8),
      child: Row(
        children: <Widget>[
          _segments(theme),
          const SizedBox(width: 16),
          Expanded(
            child: Text(
              _metaText(preview),
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
    );
  }

  /// 歌词形式的切换：逐行 / 逐字优先（没有逐字版本时后端会回退逐行）。
  Widget _segments(FluentThemeData theme) {
    final resources = theme.resources;
    Widget item(String value, String label, String hint) {
      final active = _mode == value;
      return Tooltip(
        message: hint,
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: () => unawaited(_fetch(value)),
          child: Container(
            padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 5),
            decoration: BoxDecoration(
              color: active ? resources.subtleFillColorSecondary : null,
              borderRadius: BorderRadius.circular(6),
            ),
            child: Text(
              label,
              style: TextStyle(
                fontSize: 12,
                fontWeight: active ? FontWeight.w600 : FontWeight.w400,
                color: active
                    ? resources.textFillColorPrimary
                    : resources.textFillColorSecondary,
              ),
            ),
          ),
        ),
      );
    }

    return Container(
      padding: const EdgeInsets.all(2),
      decoration: BoxDecoration(
        color: resources.subtleFillColorTertiary,
        borderRadius: BorderRadius.circular(8),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: <Widget>[
          item(_lineMode, '逐行', '普通 LRC，每行一个时间戳'),
          const SizedBox(width: 2),
          item(_wordMode, '逐字优先', '有逐字歌词时显示增强 LRC，否则回退逐行'),
        ],
      ),
    );
  }

  Widget _buildBody(FluentThemeData theme, LyricsPreviewDto? preview) {
    final resources = theme.resources;
    if (_loading) {
      return const Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            ProgressRing(),
            SizedBox(height: 12),
            Text('正在获取歌词…'),
          ],
        ),
      );
    }
    final error = _error;
    if (error != null) {
      return Padding(
        padding: const EdgeInsets.fromLTRB(20, 0, 20, 12),
        child: InfoBar(
          title: const Text('获取歌词失败'),
          content: Text(error),
          severity: InfoBarSeverity.error,
          isLong: true,
          action: Button(
            onPressed: () => unawaited(_fetch(_mode, force: true)),
            child: const Text('重试'),
          ),
        ),
      );
    }
    if (preview == null) return const SizedBox.shrink();
    if (preview.isEmpty) {
      return Center(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: <Widget>[
            Text('这首歌暂无歌词', style: theme.typography.subtitle),
            const SizedBox(height: 8),
            Text(
              '可能是纯音乐，或所选来源（${preview.source}）都没有收录这首歌的歌词。',
              textAlign: TextAlign.center,
              style: TextStyle(
                fontSize: 12,
                color: resources.textFillColorSecondary,
              ),
            ),
          ],
        ),
      );
    }
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: <Widget>[
        if (_mode == _wordMode && !preview.hasWordData)
          Padding(
            padding: const EdgeInsets.fromLTRB(20, 0, 20, 8),
            child: InfoBar(
              title: const Text('这首歌没有逐字歌词，已显示逐行歌词。'),
              severity: InfoBarSeverity.info,
              isLong: true,
            ),
          ),
        Expanded(
          child: SingleChildScrollView(
            padding: const EdgeInsets.fromLTRB(20, 0, 20, 16),
            child: SelectableText(
              preview.text,
              style: TextStyle(
                fontFamily: 'Consolas',
                fontSize: 13,
                height: 1.7,
                color: resources.textFillColorPrimary,
              ),
            ),
          ),
        ),
      ],
    );
  }

  Widget _buildFooter(FluentThemeData theme) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 12, 20, 14),
      child: Row(
        children: <Widget>[
          const Spacer(),
          Button(
            onPressed: () => Navigator.of(context).pop(),
            child: const Text('关闭'),
          ),
        ],
      ),
    );
  }
}
