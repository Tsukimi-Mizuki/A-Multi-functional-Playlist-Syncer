import 'package:fluent_ui/fluent_ui.dart';
import 'package:lunatic_princess/controller.dart';
import 'package:lunatic_princess/format.dart';
import 'package:lunatic_princess/src/rust/api/playlists.dart';

const double _rowHeight = 40;

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
        padding: const EdgeInsets.all(16),
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
        children: [
          if (d.isPartial)
            Padding(
              padding: const EdgeInsets.fromLTRB(16, 0, 16, 8),
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
          Expanded(child: _SongTable(songs: d.songs)),
        ],
      );
    }

    final subtitle = d == null
        ? ''
        : [
            if (d.creatorNickname != null) '创建者：${d.creatorNickname}',
            '${d.trackCount} 首',
          ].join('  ·  ');

    return ScaffoldPage(
      padding: EdgeInsets.zero,
      header: Padding(
        padding: const EdgeInsets.fromLTRB(16, 8, 16, 8),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              d?.name ?? _titleFallback(c),
              style: FluentTheme.of(context).typography.title,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
            if (subtitle.isNotEmpty) Text(subtitle),
          ],
        ),
      ),
      content: body,
    );
  }

  String _titleFallback(AppController c) {
    for (final p in c.playlists) {
      if (playlistKey(p.id) == c.selectedKey) return p.name;
    }
    return '歌单';
  }
}

class _SongTable extends StatelessWidget {
  const _SongTable({required this.songs});

  final List<SongDto> songs;

  @override
  Widget build(BuildContext context) {
    final dim = FluentTheme.of(context).resources.textFillColorSecondary;
    return Column(
      children: [
        SizedBox(
          height: _rowHeight,
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 24),
            child: DefaultTextStyle.merge(
              style: TextStyle(color: dim, fontWeight: FontWeight.w600),
              child: const _Columns(
                index: Text('#'),
                title: Text('标题'),
                artist: Text('歌手'),
                album: Text('专辑'),
                duration: Text('时长'),
              ),
            ),
          ),
        ),
        const Divider(),
        Expanded(
          child: ListView.builder(
            itemExtent: _rowHeight,
            itemCount: songs.length,
            itemBuilder: (context, i) => _SongRow(index: i, song: songs[i]),
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
          width: 64,
          child: Align(alignment: Alignment.centerRight, child: duration),
        ),
      ],
    );
  }
}

class _SongRow extends StatefulWidget {
  const _SongRow({required this.index, required this.song});

  final int index;
  final SongDto song;

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
    return MouseRegion(
      onEnter: (_) => setState(() => _hover = true),
      onExit: (_) => setState(() => _hover = false),
      child: Container(
        margin: const EdgeInsets.symmetric(horizontal: 12),
        padding: const EdgeInsets.symmetric(horizontal: 12),
        decoration: BoxDecoration(
          color: _hover ? res.subtleFillColorSecondary : Colors.transparent,
          borderRadius: BorderRadius.circular(4),
        ),
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
    );
  }
}
