import 'package:fluent_ui/fluent_ui.dart';

import '../controller.dart';

/// 自绘的导航侧边栏。
///
/// 不用 fluent_ui 的 [NavigationView]：在真实窗口里它的面板项（含表头与页脚项）
/// 只画出第一屏一部分，标签看不见；而 widget 测试里布局又是对的，排查成本很高。
/// 自己画一个 Column + ListView 更可控，也正好按 Win11 的观感做选择态的强调条。
class AppSidebar extends StatelessWidget {
  const AppSidebar({
    super.key,
    required this.controller,
    this.compact = false,
  });

  /// 展开态宽度，与 Win11 导航面板一致。
  static const double expandedWidth = 320;

  /// 收起态只留图标。
  static const double compactWidth = 48;

  final AppController controller;
  final bool compact;

  double get width => compact ? compactWidth : expandedWidth;

  @override
  Widget build(BuildContext context) {
    final c = controller;
    final tiles = <Widget>[
      _SidebarTile(
        icon: WindowsIcons.home,
        label: '概览',
        compact: compact,
        selected: c.selectedKey == kKeyOverview,
        onPressed: () => c.select(kKeyOverview),
      ),
      if (c.playlists.isNotEmpty) ...[
        const SizedBox(height: 12),
        _SidebarHeader(label: '我的歌单', compact: compact),
      ],
      for (final p in c.playlists)
        _SidebarTile(
          key: ValueKey(playlistKey(p.id)),
          icon: WindowsIcons.music_note,
          label: p.name,
          trailing: '${p.trackCount}',
          compact: compact,
          selected: c.selectedKey == playlistKey(p.id),
          onPressed: () => c.select(playlistKey(p.id)),
        ),
    ];

    return SizedBox(
      width: width,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const SizedBox(height: 8),
          Expanded(
            child: ListView(
              padding: EdgeInsets.symmetric(horizontal: compact ? 4 : 8),
              children: tiles,
            ),
          ),
          Padding(
            padding: EdgeInsets.symmetric(horizontal: compact ? 4 : 8),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                _SidebarTile(
                  icon: WindowsIcons.document,
                  label: '运行日志',
                  compact: compact,
                  selected: c.selectedKey == kKeyLog,
                  onPressed: () => c.select(kKeyLog),
                ),
                _SidebarTile(
                  icon: WindowsIcons.settings,
                  label: '设置',
                  compact: compact,
                  selected: c.selectedKey == kKeySettings,
                  onPressed: () => c.select(kKeySettings),
                ),
              ],
            ),
          ),
          const SizedBox(height: 8),
        ],
      ),
    );
  }
}

/// 分组标题；收起态隐藏。
class _SidebarHeader extends StatelessWidget {
  const _SidebarHeader({required this.label, required this.compact});

  final String label;
  final bool compact;

  @override
  Widget build(BuildContext context) {
    if (compact) return const SizedBox(height: 4);
    final resources = FluentTheme.of(context).resources;
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 8, 12, 4),
      child: Text(
        label,
        maxLines: 1,
        overflow: TextOverflow.ellipsis,
        style: TextStyle(
          fontSize: 12,
          fontWeight: FontWeight.w600,
          color: resources.textFillColorSecondary,
        ),
      ),
    );
  }
}

/// 一行导航项。选中时左侧有强调色小竖条 + 底色，悬停时只有底色。
class _SidebarTile extends StatefulWidget {
  const _SidebarTile({
    super.key,
    required this.icon,
    required this.label,
    required this.selected,
    required this.onPressed,
    required this.compact,
    this.trailing,
  });

  final IconData icon;
  final String label;
  final bool selected;
  final VoidCallback onPressed;
  final bool compact;

  /// 右侧的次要文字，目前用来显示歌单里的歌曲数。
  final String? trailing;

  @override
  State<_SidebarTile> createState() => _SidebarTileState();
}

class _SidebarTileState extends State<_SidebarTile> {
  bool _hovered = false;

  @override
  Widget build(BuildContext context) {
    final theme = FluentTheme.of(context);
    final resources = theme.resources;
    final selected = widget.selected;
    final background = selected
        ? resources.subtleFillColorSecondary
        : _hovered
        ? resources.subtleFillColorTertiary
        : const Color(0x00000000);
    final foreground = selected
        ? resources.textFillColorPrimary
        : resources.textFillColorSecondary;

    final row = Padding(
      padding: EdgeInsets.only(right: widget.compact ? 0 : 12),
      child: Row(
        children: [
          if (!widget.compact) const SizedBox(width: 12),
          Icon(widget.icon, size: 16, color: foreground),
          if (!widget.compact) ...[
            const SizedBox(width: 12),
            Expanded(
              child: Text(
                widget.label,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: TextStyle(
                  fontSize: 14,
                  height: 20 / 14,
                  color: foreground,
                ),
              ),
            ),
            if (widget.trailing != null) ...[
              const SizedBox(width: 8),
              Text(
                widget.trailing!,
                style: TextStyle(
                  fontSize: 12,
                  color: resources.textFillColorTertiary,
                ),
              ),
            ],
          ],
        ],
      ),
    );

    Widget tile = Container(
      height: 36,
      alignment: widget.compact ? Alignment.center : null,
      decoration: BoxDecoration(
        color: background,
        borderRadius: BorderRadius.circular(4),
      ),
      child: widget.compact
          ? row
          : Stack(
              children: [
                Positioned.fill(child: row),
                if (selected)
                  Positioned(
                    left: 0,
                    top: 10,
                    bottom: 10,
                    child: Container(
                      width: 3,
                      decoration: BoxDecoration(
                        color: theme.accentColor,
                        borderRadius: BorderRadius.circular(2),
                      ),
                    ),
                  ),
              ],
            ),
    );

    if (widget.compact) {
      tile = Tooltip(message: widget.label, child: tile);
    }

    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 1),
      child: MouseRegion(
        cursor: SystemMouseCursors.click,
        onEnter: (_) => setState(() => _hovered = true),
        onExit: (_) => setState(() => _hovered = false),
        child: GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTap: widget.onPressed,
          child: tile,
        ),
      ),
    );
  }
}