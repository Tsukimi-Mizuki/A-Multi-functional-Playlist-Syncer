import 'package:fluent_ui/fluent_ui.dart';
import 'package:flutter/services.dart';

/// 设置页风格的一行：左侧固定宽度的标签，右侧是值（可选中、可复制）。
///
/// Win11 的设置页用「标签在上、说明在下」的竖排，但概览里的运行环境是一串短
/// 标签对长路径，横排更好读，所以这里保留横排。
class InfoRow extends StatelessWidget {
  const InfoRow({
    super.key,
    required this.label,
    required this.value,
    this.monospace = false,
    this.copyable = false,
  });

  final String label;
  final String value;

  /// 路径之类的值用等宽字体，便于认出版本号与分隔符。
  final bool monospace;
  final bool copyable;

  @override
  Widget build(BuildContext context) {
    final res = FluentTheme.of(context).resources;
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          SizedBox(
            width: 96,
            child: Padding(
              padding: const EdgeInsets.only(top: 2),
              child: Text(label, style: TextStyle(color: res.textFillColorSecondary)),
            ),
          ),
          Expanded(
            child: SelectableText(
              value,
              style: monospace
                  ? const TextStyle(fontFamily: 'Consolas', fontSize: 13)
                  : null,
            ),
          ),
          if (copyable)
            Padding(
              padding: const EdgeInsets.only(left: 8),
              child: Tooltip(
                message: '复制',
                child: IconButton(
                  icon: const WindowsIcon(WindowsIcons.copy, size: 14),
                  onPressed: () => Clipboard.setData(ClipboardData(text: value)),
                ),
              ),
            ),
        ],
      ),
    );
  }
}
