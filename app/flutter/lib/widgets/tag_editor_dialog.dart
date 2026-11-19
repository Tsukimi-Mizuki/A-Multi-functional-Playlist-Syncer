import 'dart:async';
import 'dart:convert';
import 'dart:math' as math;
import 'dart:typed_data';

import 'package:fluent_ui/fluent_ui.dart';
import 'package:lunatic_princess/format.dart';
import 'package:lunatic_princess/src/rust/api/online.dart';
import 'package:lunatic_princess/src/rust/api/tags.dart';
import 'package:lunatic_princess/widgets/batch_tags_dialog.dart';
import 'package:lunatic_princess/widgets/online_tags_dialog.dart';

/// 打开「编辑标签与歌词」面板。
///
/// 对应 Tauri 版的标签编辑器：左边封面与文件信息，右边表单，下面歌词。保存时把整张表单交给
/// lp-core 的 `build_write_request` 校验与拼装，界面只负责收集文字与提示错误。
Future<void> showTagEditorDialog(BuildContext context, String path) {
  return showDialog<void>(
    context: context,
    barrierDismissible: false,
    builder: (context) => _TagEditorDialog(path: path),
  );
}

/// 封面的三种状态：原样、保存时移除、换成在线标签取回的新封面。
///
/// 从本地选图片换封面要文件对话框，排到 M8。
enum _CoverMode { keep, remove, replace }

/// 歌词来源下拉项：主词源（能带回标签与封面）与备用词源（只补歌词）。
const List<({String kind, String label})> _lyricsSources = [
  (kind: 'Netease', label: '网易云音乐'),
  (kind: 'QQMusic', label: 'QQ 音乐'),
  (kind: 'Kugou', label: '酷狗音乐（仅歌词）'),
  (kind: 'Lrclib', label: 'LRCLIB（仅歌词）'),
];

String _sourceLabel(String kind) {
  for (final entry in _lyricsSources) {
    if (entry.kind == kind) return entry.label;
  }
  return kind;
}

/// 备用词源只补歌词，不改写标签与封面。
bool _isFallbackSource(String kind) => kind == 'Kugou' || kind == 'Lrclib';

/// 一条在线搜索结果：主词源的一首歌，或备用词源的一份歌词。
class _SearchEntry {
  _SearchEntry.song(this.item, this.key) : fallback = null;

  _SearchEntry.fallback(this.fallback) : item = null, key = 'fallback-0';

  final OnlineSearchItemDto? item;
  final FallbackLyricsDto? fallback;
  final String key;
}

/// 把文件名清理成搜索关键词（口径同 lp-core 的 `keyword_from_file_name`）：
/// 去掉扩展名与开头的音轨号，下划线换空格，合并空白。
String _keywordFromFileName(String path) {
  final index = path.lastIndexOf(RegExp(r'[\\/]'));
  final base = (index < 0 ? path : path.substring(index + 1)).trim();
  final dot = base.lastIndexOf('.');
  final stem = (dot > 0 ? base.substring(0, dot) : base).trim();
  if (stem.isEmpty) return '';
  return stem
      .replaceFirst(RegExp(r'^\s*\d{1,3}[\s._-]+'), '')
      .replaceAll('_', ' ')
      .replaceAll(RegExp(r'\s+'), ' ')
      .trim();
}

/// 备用词源的歌词按偏好挑选（与 lp-core 的 `pick_lyrics` 一致：双语优先，逐字不可用回退逐行）。
String? _pickFallbackLyrics(FallbackLyricsDto data, bool preferWord) {
  bool blank(String? text) => text == null || text.trim().isEmpty;
  final translated = preferWord
      ? (data.bilingualWordLyrics ?? data.bilingualLineLyrics)
      : data.bilingualLineLyrics;
  if (!blank(translated)) return translated;
  return preferWord ? (data.wordLyrics ?? data.lineLyrics) : data.lineLyrics;
}

/// 歌词格式说明（双语 / 逐字 / 无翻译）。
String _fallbackFormatLabel(FallbackLyricsDto data, bool preferWord) {
  final base = preferWord
      ? (data.hasWordLyrics ? '逐字歌词' : '逐行歌词（该来源无逐字）')
      : '逐行歌词';
  return data.hasTranslation ? '双语$base' : '$base（该来源无翻译）';
}

/// 「歌手 · 专辑」副标题，空白字段显示「未知」。
String _songSubtitle(OnlineSongDto song) {
  final artist = song.artist.trim().isEmpty ? '未知歌手' : song.artist;
  final album = song.album.trim().isEmpty ? '未知专辑' : song.album;
  return '$artist · $album';
}

String _percent(double value) => '${(value * 100).round()}%';

String _scoreTooltip(MatchScoreDto score) =>
    '标题 ${_percent(score.title)} · 歌手 ${_percent(score.artist)} · 专辑 ${_percent(score.album)} · 时长 ${_percent(score.duration)}';

String _entryTitle(_SearchEntry entry) {
  final fallback = entry.fallback;
  if (fallback != null) {
    return fallback.title.trim().isEmpty ? '未知歌曲' : fallback.title;
  }
  return entry.item!.song.displayTitle;
}

String _entrySubtitle(_SearchEntry entry) {
  final fallback = entry.fallback;
  if (fallback != null) {
    final artist = fallback.artist.trim().isEmpty ? '未知歌手' : fallback.artist;
    return '$artist · ${fallback.sourceLabel}（仅歌词）';
  }
  return _songSubtitle(entry.item!.song);
}

String _entryMatchText(_SearchEntry entry) {
  final fallback = entry.fallback;
  if (fallback != null) return '匹配度 ${_percent(fallback.score.total)}';
  final item = entry.item!;
  final score = item.score;
  if (score == null) return '';
  return '匹配度 ${_percent(score.total)}${item.belowThreshold ? '（低于阈值）' : ''}';
}

String _entryScoreTooltip(_SearchEntry entry) {
  final fallback = entry.fallback;
  if (fallback != null) return _scoreTooltip(fallback.score);
  final score = entry.item!.score;
  return score == null ? '' : _scoreTooltip(score);
}

class _TagEditorDialog extends StatefulWidget {
  const _TagEditorDialog({required this.path});

  final String path;

  @override
  State<_TagEditorDialog> createState() => _TagEditorDialogState();
}

class _TagEditorDialogState extends State<_TagEditorDialog> {
  static const _keys = [
    'title',
    'artist',
    'album',
    'albumArtist',
    'year',
    'trackNumber',
    'discNumber',
    'genre',
    'composer',
    'lyricist',
    'comment',
    'lyrics',
  ];

  late final Map<String, TextEditingController> _controllers = {
    for (final key in _keys) key: TextEditingController(),
  };

  bool _loading = true;
  bool _saving = false;
  TagViewDto? _view;
  EditorFieldsDto? _baseline;
  String? _loadError;
  String? _saveError;
  String? _notice;
  _CoverMode _cover = _CoverMode.keep;

  /// 在线标签取回的新封面，`_cover == replace` 时保存才写入。
  CoverInfoDto? _newCover;

  /// 新封面的来源名，用于界面文案。
  String? _newCoverOrigin;

  /// 在线标签给的艺术家名单：保存时优先用它写多值；用户改过艺术家就不再用。
  ArtistListDto? _onlineArtists;

  /// 记下在线标签填进来的艺术家文字，用来判断用户有没有改过。
  String? _onlineArtistsText;

  /// 在线标签刚回填时的提示，保存或重新读取后清掉。
  String? _onlineNote;

  /// 在线搜索：来源、是否优先逐字、关键词、进行中、状态行、结果与选中项。
  String _source = 'Netease';
  bool _preferWord = false;
  late final TextEditingController _keyword = TextEditingController(
    text: _keywordFromFileName(widget.path),
  );
  bool _searching = false;
  String _searchStatus = '';
  List<_SearchEntry> _results = const [];
  String? _selectedKey;
  String _thresholdText = '60%';

  /// 用来丢弃过期的搜索结果与过期的载入结果。
  int _searchToken = 0;
  int _applyToken = 0;

  /// 打开文件后自动按文件名搜一次（与 Tauri 版一致）；只做一次。
  bool _autoSearched = false;
  bool _optionsLoaded = false;

  @override
  void initState() {
    super.initState();
    _load();
    _loadEditorOptions();
  }

  @override
  void dispose() {
    for (final controller in _controllers.values) {
      controller.dispose();
    }
    _keyword.dispose();
    super.dispose();
  }

  String _text(String key) => _controllers[key]!.text;

  EditorFieldsDto _collect() => EditorFieldsDto(
    title: _text('title'),
    artist: _text('artist'),
    album: _text('album'),
    albumArtist: _text('albumArtist'),
    year: _text('year'),
    trackNumber: _text('trackNumber'),
    discNumber: _text('discNumber'),
    genre: _text('genre'),
    composer: _text('composer'),
    lyricist: _text('lyricist'),
    comment: _text('comment'),
    lyrics: _text('lyrics'),
  );

  void _fill(EditorFieldsDto fields) {
    _controllers['title']!.text = fields.title;
    _controllers['artist']!.text = fields.artist;
    _controllers['album']!.text = fields.album;
    _controllers['albumArtist']!.text = fields.albumArtist;
    _controllers['year']!.text = fields.year;
    _controllers['trackNumber']!.text = fields.trackNumber;
    _controllers['discNumber']!.text = fields.discNumber;
    _controllers['genre']!.text = fields.genre;
    _controllers['composer']!.text = fields.composer;
    _controllers['lyricist']!.text = fields.lyricist;
    _controllers['comment']!.text = fields.comment;
    _controllers['lyrics']!.text = fields.lyrics;
  }

  bool _same(EditorFieldsDto a, EditorFieldsDto b) =>
      a.title == b.title &&
      a.artist == b.artist &&
      a.album == b.album &&
      a.albumArtist == b.albumArtist &&
      a.year == b.year &&
      a.trackNumber == b.trackNumber &&
      a.discNumber == b.discNumber &&
      a.genre == b.genre &&
      a.composer == b.composer &&
      a.lyricist == b.lyricist &&
      a.comment == b.comment &&
      a.lyrics == b.lyrics;

  bool get _dirty {
    final baseline = _baseline;
    if (baseline == null) return false;
    if (_cover != _CoverMode.keep) return true;
    return !_same(_collect(), baseline);
  }

  Future<void> _load({bool quiet = false}) async {
    setState(() {
      _loading = true;
      _loadError = null;
      _results = const [];
      _searchStatus = '';
      _selectedKey = null;
      _searchToken += 1;
      _applyToken += 1;
      if (!quiet) {
        _notice = null;
        _saveError = null;
        _onlineNote = null;
      }
    });
    try {
      final result = await tagEditorOpen(path: widget.path);
      if (!mounted) return;
      final error = result.error;
      if (error != null) {
        setState(() {
          _loading = false;
          _loadError = error.message;
        });
        return;
      }
      final view = result.view;
      final fields = result.fields;
      if (view == null || fields == null) {
        setState(() {
          _loading = false;
          _loadError = '无法读取该文件：格式不支持或文件已损坏。';
        });
        return;
      }
      setState(() {
        _loading = false;
        _view = view;
        _baseline = fields;
        _cover = _CoverMode.keep;
        _newCover = null;
        _newCoverOrigin = null;
        _onlineArtists = null;
        _onlineArtistsText = null;
        _fill(fields);
      });
      _maybeAutoSearch();
    } catch (error) {
      if (!mounted) return;
      setState(() {
        _loading = false;
        _loadError = '读取标签失败：$error';
      });
    }
  }

  Future<void> _save() async {
    final baseline = _baseline;
    if (baseline == null || _saving || _loading) return;
    setState(() {
      _saving = true;
      _saveError = null;
      _notice = null;
    });
    try {
      final newCover = _cover == _CoverMode.replace ? _newCover : null;
      // 艺术家被改过以后，在线给的多值名单就作废了，保存时按文字重新拆分。
      final online = _onlineArtists != null && _text('artist') == _onlineArtistsText
          ? _onlineArtists
          : null;
      final result = await tagEditorSave(
        path: widget.path,
        fields: _collect(),
        loaded: baseline,
        online: online,
        cover: switch (_cover) {
          _CoverMode.remove => CoverChangeDto(kind: 'remove'),
          _CoverMode.replace when newCover != null => CoverChangeDto(
            kind: 'replace',
            base64: newCover.base64,
            mimeType: newCover.mimeType,
          ),
          _ => CoverChangeDto(kind: 'keep'),
        },
      );
      if (!mounted) return;
      final error = result.error;
      if (error != null) {
        setState(() {
          _saving = false;
          _saveError = error.message;
        });
        return;
      }
      final view = result.view;
      final fields = result.fields;
      if (view == null || fields == null) {
        setState(() {
          _saving = false;
          _saveError = '保存成功，但重新读取标签失败。';
        });
        return;
      }
      setState(() {
        _saving = false;
        _view = view;
        _baseline = fields;
        _cover = _CoverMode.keep;
        _newCover = null;
        _newCoverOrigin = null;
        _onlineArtists = null;
        _onlineArtistsText = null;
        _onlineNote = null;
        _fill(fields);
        _notice = '已保存：${view.fileName}';
      });
    } catch (error) {
      if (!mounted) return;
      setState(() {
        _saving = false;
        _saveError = '保存标签失败：$error';
      });
    }
  }

  Future<bool> _confirmDiscard(String message) async {
    final discard = await showDialog<bool>(
      context: context,
      builder: (dialogContext) => ContentDialog(
        title: const Text('未保存的修改'),
        content: Text(message),
        actions: [
          Button(
            onPressed: () => Navigator.of(dialogContext).pop(false),
            child: const Text('继续编辑'),
          ),
          FilledButton(
            onPressed: () => Navigator.of(dialogContext).pop(true),
            child: const Text('放弃修改'),
          ),
        ],
      ),
    );
    return discard ?? false;
  }

  Future<void> _reload() async {
    if (_loading || _saving) return;
    if (_dirty && !await _confirmDiscard('重新读取会丢弃未保存的修改，继续吗？')) {
      return;
    }
    if (!mounted) return;
    await _load(quiet: true);
  }

  Future<void> _requestClose() async {
    if (_saving) return;
    if (_dirty && !await _confirmDiscard('标签改动还没有保存，关闭后会丢失。')) {
      return;
    }
    if (!mounted) return;
    Navigator.of(context).pop();
  }

  @override
  Widget build(BuildContext context) {
    final theme = FluentTheme.of(context);
    final screen = MediaQuery.sizeOf(context);
    final width = math.min(1020.0, screen.width - 64);
    final height = math.min(760.0, screen.height - 64);
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
              if (_notice != null || _saveError != null || _onlineNote != null)
                _buildBanners(),
              Expanded(child: _buildBody(theme)),
              Divider(style: DividerThemeData(horizontalMargin: EdgeInsets.zero)),
              _buildFooter(),
            ],
          ),
        ),
      ),
    );
  }

  Widget _buildHeader(FluentThemeData theme) {
    final res = theme.resources;
    final view = _view;
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 12, 8, 12),
      child: Row(
        children: [
          Icon(WindowsIcons.edit, size: 18, color: res.textFillColorSecondary),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisSize: MainAxisSize.min,
              children: [
                Text('编辑标签与歌词', style: theme.typography.subtitle),
                const SizedBox(height: 2),
                Text(
                  view?.fileName ?? widget.path,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 12,
                    color: res.textFillColorSecondary,
                  ),
                ),
              ],
            ),
          ),
          if (_dirty)
            Padding(
              padding: const EdgeInsets.only(right: 8),
              child: Text(
                '未保存',
                style: TextStyle(
                  fontSize: 12,
                  color: res.systemFillColorCaution,
                ),
              ),
            ),
          IconButton(
            icon: const Icon(WindowsIcons.cancel, size: 14),
            onPressed: _saving ? null : _requestClose,
          ),
        ],
      ),
    );
  }

  Widget _buildBanners() {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 10, 20, 0),
      child: Column(
        children: [
          if (_onlineNote != null)
            Padding(
              padding: const EdgeInsets.only(bottom: 8),
              child: InfoBar(
                severity: InfoBarSeverity.info,
                title: const Text('在线标签'),
                content: Text(_onlineNote!),
              ),
            ),
          if (_saveError != null)
            InfoBar(
              severity: InfoBarSeverity.error,
              title: const Text('保存失败'),
              content: Text(_saveError!),
            ),
          if (_notice != null)
            InfoBar(
              severity: InfoBarSeverity.success,
              title: const Text('已保存'),
              content: Text(_notice!),
            ),
        ],
      ),
    );
  }

  Widget _buildBody(FluentThemeData theme) {
    if (_loading) {
      return const Center(child: ProgressRing());
    }
    final error = _loadError;
    if (error != null) {
      return Center(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: InfoBar(
            severity: InfoBarSeverity.error,
            title: const Text('无法读取标签'),
            content: Text(error),
            action: Button(onPressed: _load, child: const Text('重试')),
          ),
        ),
      );
    }
    final view = _view;
    if (view == null) return const SizedBox.shrink();
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 16, 20, 16),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(width: 220, child: _buildCoverColumn(theme, view)),
          const SizedBox(width: 20),
          Expanded(child: _buildFieldsColumn(theme, view)),
        ],
      ),
    );
  }

  Widget _buildFieldsColumn(FluentThemeData theme, TagViewDto view) {
    final res = theme.resources;
    final hint = view.sidecarLyrics != null
        ? '来自同名歌词文件「${view.sidecarLyrics!.fileName}」，保存时写进音频文件'
        : view.lyrics != null
        ? '内嵌在音频文件里'
        : '没有歌词';
    return SingleChildScrollView(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          _buildSearchPanel(theme),
          Row(
            children: [
              const Text('标签信息', style: TextStyle(fontWeight: FontWeight.w600)),
              const Spacer(),
              Button(
                onPressed: _loading || _saving ? null : _openOnline,
                child: Row(
                  mainAxisSize: MainAxisSize.min,
                  children: const [
                    Icon(WindowsIcons.search, size: 14),
                    SizedBox(width: 6),
                    Text('在线标签…'),
                  ],
                ),
              ),
            ],
          ),
          const SizedBox(height: 10),
          _pair(_textField('title', '标题')),
          _pair(_textField('artist', '艺术家（多个用 / 分隔）')),
          _pair(_textField('album', '专辑')),
          _pair(_textField('albumArtist', '专辑艺术家')),
          _pair(_textField('year', '年份')),
          _pair(_textField('trackNumber', '音轨号')),
          _pair(_textField('discNumber', '碟号')),
          _pair(_textField('genre', '流派')),
          _pair(_textField('composer', '作曲家')),
          _pair(_textField('lyricist', '作词家')),
          _pair(_textField('comment', '备注')),
          const SizedBox(height: 6),
          Row(
            children: [
              Text('歌词', style: TextStyle(fontWeight: FontWeight.w600)),
              const SizedBox(width: 8),
              Expanded(
                child: Text(
                  hint,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: TextStyle(
                    fontSize: 12,
                    color: res.textFillColorSecondary,
                  ),
                ),
              ),
            ],
          ),
          const SizedBox(height: 6),
          TextBox(
            controller: _controllers['lyrics'],
            maxLines: 10,
            minLines: 6,
            style: const TextStyle(fontFamily: 'Consolas', fontSize: 13),
          ),
        ],
      ),
    );
  }

  Widget _pair(Widget? first, [Widget? second]) {
    return Padding(
      padding: const EdgeInsets.only(bottom: 10),
      child: Row(
        children: [
          Expanded(child: first ?? const SizedBox.shrink()),
          if (second != null) ...[
            const SizedBox(width: 12),
            Expanded(child: second),
          ],
        ],
      ),
    );
  }

  Widget _textField(String key, String label) {
    final res = FluentTheme.of(context).resources;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          label,
          style: TextStyle(fontSize: 12, color: res.textFillColorSecondary),
        ),
        const SizedBox(height: 4),
        TextBox(controller: _controllers[key]),
      ],
    );
  }

  Widget _buildCoverColumn(FluentThemeData theme, TagViewDto view) {
    final res = theme.resources;
    final newCover = _cover == _CoverMode.replace ? _newCover : null;
    final caption = switch (_cover) {
      _CoverMode.replace when newCover != null =>
        '新封面：${_newCoverOrigin ?? '在线标签'}（保存后写入）',
      _CoverMode.remove => '封面将在保存时移除',
      _CoverMode.keep when !view.hasCover => '没有封面',
      _CoverMode.keep when view.coverDataUrl == null => '封面过大，未预览（保存时保持原样）',
      _CoverMode.keep => '当前封面（保存时保持原样）',
      _ => '没有封面',
    };
    final meta = newCover != null
        ? [
            newCover.mimeType,
            if (newCover.width != null && newCover.height != null)
              '${newCover.width}×${newCover.height}',
            formatFileSize(newCover.size),
          ].join('  ·  ')
        : [
            if (view.coverMimeType != null) view.coverMimeType!,
            if (view.coverWidth != null && view.coverHeight != null)
              '${view.coverWidth}×${view.coverHeight}',
            if (view.coverSize != null) formatFileSize(view.coverSize!),
            if (view.coverPictureType != null) view.coverPictureType!,
          ].join('  ·  ');
    return SingleChildScrollView(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          _coverImage(theme, view),
          const SizedBox(height: 8),
          Text(
            caption,
            style: TextStyle(fontSize: 12, color: res.textFillColorSecondary),
          ),
          if (meta.isNotEmpty) ...[
            const SizedBox(height: 4),
            Text(
              meta,
              style: TextStyle(fontSize: 12, color: res.textFillColorSecondary),
            ),
          ],
          const SizedBox(height: 8),
          if (_cover == _CoverMode.replace)
            Button(
              onPressed: _saving
                  ? null
                  : () => setState(() {
                      _cover = _CoverMode.keep;
                      _newCover = null;
                      _newCoverOrigin = null;
                    }),
              child: const Text('取消更换封面'),
            )
          else if (view.hasCover && _cover == _CoverMode.keep)
            Button(
              onPressed: _saving
                  ? null
                  : () => setState(() => _cover = _CoverMode.remove),
              child: const Text('移除封面'),
            )
          else if (_cover == _CoverMode.remove)
            Button(
              onPressed: _saving
                  ? null
                  : () => setState(() => _cover = _CoverMode.keep),
              child: const Text('恢复封面'),
            ),
          const SizedBox(height: 16),
          _infoLine(theme, '格式', view.fileFormat ?? '未知'),
          _infoLine(theme, '时长', formatDuration(view.durationMs)),
          _infoLine(theme, '码率', '${view.bitrate} kbps'),
          _infoLine(theme, '大小', formatFileSize(view.fileSize)),
          _infoLine(theme, '网易云 ID', view.neteaseId ?? '无'),
          const SizedBox(height: 8),
          Text(
            '路径',
            style: TextStyle(fontSize: 12, color: res.textFillColorSecondary),
          ),
          const SizedBox(height: 2),
          SelectableText(
            view.path,
            style: TextStyle(fontSize: 12, color: res.textFillColorSecondary),
          ),
        ],
      ),
    );
  }

  Widget _coverImage(FluentThemeData theme, TagViewDto view) {
    final res = theme.resources;
    Widget placeholder(IconData icon, String label) => Container(
      width: 200,
      height: 200,
      decoration: BoxDecoration(
        color: res.subtleFillColorSecondary,
        borderRadius: BorderRadius.circular(8),
      ),
      child: Column(
        mainAxisAlignment: MainAxisAlignment.center,
        children: [
          Icon(icon, size: 32, color: res.textFillColorSecondary),
          const SizedBox(height: 8),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12),
            child: Text(
              label,
              textAlign: TextAlign.center,
              style: TextStyle(fontSize: 12, color: res.textFillColorSecondary),
            ),
          ),
        ],
      ),
    );
    final newCover = _cover == _CoverMode.replace ? _newCover : null;
    final newBytes = newCover == null ? null : _decodeBase64(newCover.base64);
    if (newBytes != null) {
      return ClipRRect(
        borderRadius: BorderRadius.circular(8),
        child: Image.memory(
          newBytes,
          width: 200,
          height: 200,
          fit: BoxFit.cover,
          gaplessPlayback: true,
          errorBuilder: (_, _, _) => placeholder(WindowsIcons.picture, '新封面无法预览'),
        ),
      );
    }
    if (_cover == _CoverMode.remove || !view.hasCover) {
      return placeholder(
        WindowsIcons.cancel,
        _cover == _CoverMode.remove ? '保存时移除封面' : '没有封面',
      );
    }
    final dataUrl = view.coverDataUrl;
    final bytes = dataUrl == null ? null : _decodeCover(dataUrl);
    if (bytes == null) {
      return placeholder(WindowsIcons.picture, '封面无法预览');
    }
    return ClipRRect(
      borderRadius: BorderRadius.circular(8),
      child: Image.memory(
        bytes,
        width: 200,
        height: 200,
        fit: BoxFit.cover,
        gaplessPlayback: true,
        errorBuilder: (_, _, _) => placeholder(WindowsIcons.picture, '封面无法预览'),
      ),
    );
  }

  Uint8List? _decodeCover(String dataUrl) {
    final comma = dataUrl.indexOf(',');
    if (comma < 0) return null;
    return _decodeBase64(dataUrl.substring(comma + 1));
  }

  Uint8List? _decodeBase64(String base64) {
    try {
      return base64Decode(base64);
    } catch (_) {
      return null;
    }
  }

  Widget _infoLine(FluentThemeData theme, String label, String value) {
    final res = theme.resources;
    return Padding(
      padding: const EdgeInsets.only(bottom: 6),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 64,
            child: Text(
              label,
              style: TextStyle(fontSize: 12, color: res.textFillColorSecondary),
            ),
          ),
          Expanded(child: SelectableText(value, style: const TextStyle(fontSize: 12))),
        ],
      ),
    );
  }

  /// 读取上次用的搜索来源与歌词格式，并把匹配度阈值显示出来。
  Future<void> _loadEditorOptions() async {
    try {
      final options = await editorOptions();
      if (!mounted) return;
      setState(() {
        _source = options.searchSource;
        _preferWord = options.preferWordLyrics;
        _thresholdText = options.thresholdPercent;
      });
    } catch (_) {
      // 读不到就用默认值：网易云音乐、逐行歌词、阈值 60%。
    } finally {
      _optionsLoaded = true;
      _maybeAutoSearch();
    }
  }

  /// 文件与编辑器选项都就绪后，按关键词自动搜一次（Tauri 版打开文件就搜）。
  void _maybeAutoSearch() {
    if (_autoSearched || !_optionsLoaded || _view == null) return;
    final keyword = _keyword.text.trim();
    if (keyword.isEmpty) return;
    _autoSearched = true;
    unawaited(_runSearch(keyword));
  }

  /// 记住这次选的来源与歌词格式；记忆失败不影响编辑。
  Future<void> _rememberOptions() async {
    try {
      await editorRememberOptions(
        source: _source,
        preferWordLyrics: _preferWord,
      );
    } catch (_) {
      // 忽略：选项只影响下次打开时的默认值。
    }
  }

  /// 打分用的本地文件信息：取表单里现在的标题、歌手、专辑与文件时长。
  FileContextDto _fileContext() {
    final title = _text('title').trim();
    final artist = _text('artist').trim();
    final album = _text('album').trim();
    return FileContextDto(
      title: title.isEmpty ? null : title,
      artist: artist.isEmpty ? null : artist,
      album: album.isEmpty ? null : album,
      durationMs: _view?.durationMs ?? 0,
      filePath: widget.path,
    );
  }

  /// 主词源按关键词搜歌；备用词源按当前音频的标题、歌手与时长检索一份歌词。
  Future<void> _runSearch(String rawKeyword) async {
    final token = ++_searchToken;
    _applyToken += 1;
    setState(() {
      _results = const [];
      _selectedKey = null;
    });
    final view = _view;
    if (_isFallbackSource(_source)) {
      final label = _sourceLabel(_source);
      if (view == null) {
        setState(() {
          _searching = false;
          _searchStatus = '备用词源按当前音频的标题、歌手和时长检索，请先打开要编辑的音频文件。';
        });
        return;
      }
      final title = _text('title').trim();
      final artist = _text('artist').trim();
      final album = _text('album').trim();
      setState(() {
        _searching = true;
        _searchStatus = '正在从$label检索歌词...';
      });
      try {
        final outcome = await lyricsSourceFetch(
          source: _source,
          title: title.isEmpty ? _keywordFromFileName(widget.path) : title,
          artist: artist,
          durationMs: view.durationMs,
          album: album.isEmpty ? null : album,
          wordLyrics: _preferWord,
        );
        if (!mounted || token != _searchToken) return;
        final error = outcome.error;
        if (error != null) {
          setState(() => _searchStatus = '$label检索失败：${error.message}');
          return;
        }
        final data = outcome.data;
        if (data == null) {
          setState(
            () => _searchStatus = '没有在$label找到匹配的歌词；可以改用网易云音乐 / QQ 音乐按关键词搜索。',
          );
          return;
        }
        setState(() {
          _results = [_SearchEntry.fallback(data)];
          _selectedKey = _results.first.key;
          _searchStatus = '${data.sourceLabel}找到了 1 份歌词，点击结果载入（仅歌词，标签与封面保持当前内容）。';
        });
      } catch (error) {
        if (mounted && token == _searchToken) {
          setState(() => _searchStatus = '$label检索失败：$error');
        }
      } finally {
        if (mounted && token == _searchToken) {
          setState(() => _searching = false);
        }
      }
      return;
    }
    final keyword = rawKeyword.trim();
    if (keyword.isEmpty) {
      setState(() {
        _searching = false;
        _searchStatus = '请输入要搜索的歌曲、歌手或专辑。';
      });
      return;
    }
    setState(() {
      _searching = true;
      _searchStatus = '正在搜索...';
    });
    try {
      final outcome = await onlineSearch(
        source: _source,
        query: keyword,
        fileContext: _fileContext(),
      );
      if (!mounted || token != _searchToken) return;
      final error = outcome.error;
      if (error != null) {
        setState(() => _searchStatus = '搜索失败：${error.message}');
        return;
      }
      final data = outcome.data;
      if (data == null) {
        setState(() => _searchStatus = '搜索失败：没有返回内容，请重试。');
        return;
      }
      final entries = [
        for (var i = 0; i < data.items.length; i++)
          _SearchEntry.song(
            data.items[i],
            '${data.items[i].song.source}:${data.items[i].song.sourceId}:$i',
          ),
      ];
      setState(() {
        _results = entries;
        _selectedKey = null;
        _searchStatus = entries.isEmpty
            ? '没有找到匹配歌曲，换个关键词试试。'
            : data.scored
            ? '找到 ${entries.length} 首歌曲，按匹配度从高到低排序（低于 ${data.thresholdPercent}% 的已标注）。点击结果载入标签和歌词。'
            : '找到 ${entries.length} 首歌曲，点击结果即可载入标签和歌词。';
      });
    } catch (error) {
      if (mounted && token == _searchToken) {
        setState(() => _searchStatus = '搜索失败：$error');
      }
    } finally {
      if (mounted && token == _searchToken) {
        setState(() => _searching = false);
      }
    }
  }

  /// 把一条搜索结果填进表单：主词源填标签 / 歌词 / 封面，备用词源只填歌词。
  Future<void> _applyEntry(_SearchEntry entry) async {
    if (_loading || _saving) return;
    final token = ++_applyToken;
    setState(() => _selectedKey = entry.key);
    final fallback = entry.fallback;
    if (fallback != null) {
      final lyrics = _pickFallbackLyrics(fallback, _preferWord);
      if (lyrics == null || lyrics.trim().isEmpty) {
        setState(() => _searchStatus = '${fallback.sourceLabel}没有返回可用歌词。');
        return;
      }
      setState(() {
        _controllers['lyrics']!.text = lyrics;
        _searchStatus =
            '已从${fallback.sourceLabel}载入${_fallbackFormatLabel(fallback, _preferWord)}（仅歌词，标签与封面保持当前内容），请确认后保存。';
      });
      return;
    }
    final summary = entry.item!.song;
    setState(() => _searchStatus = '正在获取${summary.sourceLabel}的标签和歌词...');
    try {
      final outcome = await onlineLoadResult(
        source: summary.source,
        id: summary.sourceId,
        wordLyrics: _preferWord,
        song: summary,
      );
      if (!mounted || token != _applyToken) return;
      final error = outcome.error;
      if (error != null) {
        setState(() => _searchStatus = '获取在线信息失败：${error.message}');
        return;
      }
      final loaded = outcome.data;
      if (loaded == null) {
        setState(() => _searchStatus = '获取在线信息失败：没有返回内容。');
        return;
      }
      final song = loaded.song;
      setState(() {
        _applyOnlineTags(song);
        // 来源给了歌手名单就记下来：保存时按它写多值，用户改过艺术家就作废。
        _onlineArtists = ArtistListDto(text: _text('artist'), values: song.artists);
        _onlineArtistsText = _text('artist');
        final lyrics = loaded.lyrics;
        if (lyrics != null && lyrics.trim().isNotEmpty) {
          _controllers['lyrics']!.text = lyrics;
        }
        final cover = loaded.cover;
        if (cover != null) {
          _newCover = cover;
          _newCoverOrigin = song.sourceLabel;
          _cover = _CoverMode.replace;
        }
        _searchStatus = loaded.statusText;
      });
    } catch (error) {
      if (mounted && token == _applyToken) {
        setState(() => _searchStatus = '获取在线信息失败：$error');
      }
    }
  }

  /// 在线搜索面板：来源、关键词、歌词格式、搜索按钮、状态行与结果列表。
  Widget _buildSearchPanel(FluentThemeData theme) {
    final res = theme.resources;
    final busy = _searching || _loading || _saving;
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Row(
          children: [
            const Text('在线搜索', style: TextStyle(fontWeight: FontWeight.w600)),
            const SizedBox(width: 8),
            Expanded(
              child: Text(
                '网易云音乐、QQ 音乐能带回标签与封面；酷狗、LRCLIB 只补歌词。',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(fontSize: 12, color: res.textFillColorSecondary),
              ),
            ),
          ],
        ),
        const SizedBox(height: 8),
        Row(
          children: [
            SizedBox(
              width: 150,
              child: ComboBox<String>(
                value: _source,
                items: [
                  for (final item in _lyricsSources)
                    ComboBoxItem(value: item.kind, child: Text(item.label)),
                ],
                onChanged: (value) {
                  if (value == null || value == _source) return;
                  setState(() => _source = value);
                  unawaited(_rememberOptions());
                },
              ),
            ),
            const SizedBox(width: 8),
            Expanded(
              child: TextBox(
                controller: _keyword,
                placeholder: '搜索歌曲、歌手或专辑，例如：夜空中最亮的星',
                onSubmitted: (value) => unawaited(_runSearch(value)),
              ),
            ),
            const SizedBox(width: 8),
            SizedBox(
              width: 150,
              child: ComboBox<String>(
                value: _preferWord ? 'word' : 'line',
                items: const [
                  ComboBoxItem(value: 'line', child: Text('逐行歌词')),
                  ComboBoxItem(value: 'word', child: Text('逐字歌词（有则优先）')),
                ],
                onChanged: (value) {
                  if (value == null) return;
                  final preferWord = value == 'word';
                  if (preferWord == _preferWord) return;
                  setState(() => _preferWord = preferWord);
                  unawaited(_rememberOptions());
                },
              ),
            ),
            const SizedBox(width: 8),
            FilledButton(
              onPressed: busy ? null : () => unawaited(_runSearch(_keyword.text)),
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
                  Text(_searching ? '搜索中…' : '搜索'),
                ],
              ),
            ),
          ],
        ),
        if (_searchStatus.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Text(
              _searchStatus,
              style: TextStyle(fontSize: 12, color: res.textFillColorSecondary),
            ),
          ),
        if (_results.isNotEmpty)
          Padding(
            padding: const EdgeInsets.only(top: 8),
            child: Container(
              constraints: const BoxConstraints(maxHeight: 168),
              decoration: BoxDecoration(
                border: Border.all(
                  color: res.dividerStrokeColorDefault,
                  width: 0.5,
                ),
                borderRadius: BorderRadius.circular(6),
              ),
              child: ListView.builder(
                shrinkWrap: true,
                padding: EdgeInsets.zero,
                itemCount: _results.length,
                itemBuilder: (context, index) =>
                    _buildResultTile(theme, _results[index]),
              ),
            ),
          ),
        Padding(
          padding: const EdgeInsets.only(top: 6),
          child: Text(
            '匹配度阈值 $_thresholdText（在设置的「在线匹配度」里调整）',
            style: TextStyle(fontSize: 11.5, color: res.textFillColorSecondary),
          ),
        ),
        const SizedBox(height: 14),
      ],
    );
  }

  Widget _buildResultTile(FluentThemeData theme, _SearchEntry entry) {
    final res = theme.resources;
    final selected = entry.key == _selectedKey;
    final matchText = _entryMatchText(entry);
    final scoreTitle = _entryScoreTooltip(entry);
    final below = entry.item?.belowThreshold ?? false;
    final fallback = entry.fallback;
    final durationText = fallback != null
        ? formatDuration(fallback.durationMs)
        : entry.item!.song.durationText;
    final match = Text(
      matchText,
      style: TextStyle(
        fontSize: 11.5,
        color: below ? res.systemFillColorCaution : res.textFillColorSecondary,
      ),
    );
    return GestureDetector(
      behavior: HitTestBehavior.opaque,
      onTap: () => unawaited(_applyEntry(entry)),
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        decoration: BoxDecoration(
          color: selected ? res.subtleFillColorSecondary : null,
          border: Border(
            bottom: BorderSide(
              color: res.dividerStrokeColorDefault,
              width: 0.5,
            ),
          ),
        ),
        child: Row(
          children: [
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisSize: MainAxisSize.min,
                children: [
                  Text(
                    _entryTitle(entry),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(
                      fontSize: 13,
                      fontWeight: FontWeight.w600,
                    ),
                  ),
                  const SizedBox(height: 2),
                  Text(
                    _entrySubtitle(entry),
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: TextStyle(
                      fontSize: 11.5,
                      color: res.textFillColorSecondary,
                    ),
                  ),
                ],
              ),
            ),
            if (matchText.isNotEmpty) ...[
              const SizedBox(width: 8),
              scoreTitle.isEmpty
                  ? match
                  : Tooltip(message: scoreTitle, child: match),
            ],
            const SizedBox(width: 8),
            Text(
              durationText,
              style: TextStyle(
                fontSize: 11.5,
                color: res.textFillColorSecondary,
              ),
            ),
          ],
        ),
      ),
    );
  }

  /// 打开「在线标签」：关键词与文件上下文都按当前表单内容冻结。
  ///
  /// 标题为空时关键词也留空，交给 lp-core 按文件路径推。
  Future<void> _openOnline() async {
    final view = _view;
    if (view == null || _saving || _loading) return;
    final title = _text('title').trim();
    final artist = _text('artist').trim();
    final album = _text('album').trim();
    final query = title.isEmpty
        ? ''
        : [
            title,
            ...artist
                .split(RegExp(r'[/;；、]'))
                .map((part) => part.trim())
                .where((part) => part.isNotEmpty),
          ].join(' ');
    final result = await showOnlineTagsDialog(
      context,
      query: query,
      fileContext: FileContextDto(
        title: title.isEmpty ? null : title,
        artist: artist.isEmpty ? null : artist,
        album: album.isEmpty ? null : album,
        durationMs: view.durationMs,
        filePath: view.path,
      ),
    );
    if (!mounted || result == null) return;
    final loaded = result.loaded;
    final song = loaded.song;
    final cover = loaded.cover;
    setState(() {
      if (result.tagsRequested) {
        _applyOnlineTags(song);
        _onlineArtists = ArtistListDto(
          text: _text('artist'),
          values: song.artists,
        );
        _onlineArtistsText = _text('artist');
      }
      if (result.coverRequested && cover != null) {
        _newCover = cover;
        _newCoverOrigin = song.sourceLabel;
        _cover = _CoverMode.replace;
      }
      final what = result.tagsRequested && result.coverRequested
          ? '标签与封面'
          : (result.tagsRequested ? '标签' : '封面');
      final missingCover = result.tagsRequested && cover == null
          ? '（没有获取到封面）'
          : '';
      _onlineNote =
          '已载入${song.sourceLabel}的$what$missingCover（歌词没有改动），请确认后保存。';
    });
  }

  /// 把在线候选项里非空的字段填进表单；空白字段保持原值，不清空。
  void _applyOnlineTags(OnlineSongDto song) {
    void put(String key, String? value) {
      final text = value?.trim() ?? '';
      if (text.isEmpty) return;
      _controllers[key]!.text = text;
    }

    put('title', song.title);
    put('artist', song.artist);
    put('album', song.album);
    put('year', song.year?.toString());
    put('trackNumber', song.trackNumber?.toString());
    put('discNumber', song.discNumber?.toString());
    put('genre', song.genre);
    put('composer', song.composer);
  }

  /// 打开批量修改标签：从当前文件所在的目录开始列。
  void _openBatch() {
    final index = widget.path.lastIndexOf(RegExp(r'[\\/]'));
    final folder = index <= 0 ? '' : widget.path.substring(0, index);
    unawaited(showBatchTagsDialog(context, initialFolder: folder));
  }

  Widget _buildFooter() {
    return Padding(
      padding: const EdgeInsets.fromLTRB(20, 10, 20, 10),
      child: Row(
        children: [
          Button(
            onPressed: _loading || _saving ? null : _openBatch,
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: const [
                Icon(WindowsIcons.edit, size: 14),
                SizedBox(width: 6),
                Text('批量修改'),
              ],
            ),
          ),
          const SizedBox(width: 8),
          Button(
            onPressed: _loading || _saving ? null : _reload,
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: const [
                Icon(WindowsIcons.refresh, size: 14),
                SizedBox(width: 6),
                Text('重新读取'),
              ],
            ),
          ),
          const Spacer(),
          Button(
            onPressed: _saving ? null : _requestClose,
            child: const Text('关闭'),
          ),
          const SizedBox(width: 10),
          FilledButton(
            onPressed: _saving || _loading || _loadError != null ? null : _save,
            child: Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (_saving)
                  const Padding(
                    padding: EdgeInsets.only(right: 8),
                    child: SizedBox(
                      width: 14,
                      height: 14,
                      child: ProgressRing(),
                    ),
                  ),
                const Text('保存'),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
