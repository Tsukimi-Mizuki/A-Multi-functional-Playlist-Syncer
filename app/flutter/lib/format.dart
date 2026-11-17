/// 把毫秒时长格式化成 `m:ss`；无效（<= 0）时显示 `--:--`。
String formatDuration(int ms) {
  if (ms <= 0) return '--:--';
  final totalSeconds = (ms / 1000).round();
  final m = totalSeconds ~/ 60;
  final s = totalSeconds % 60;
  return '$m:${s.toString().padLeft(2, '0')}';
}

/// 字节数 -> `10.3 MB` 这样的可读大小（文件大小、封面大小都用它）。
String formatFileSize(int bytes) {
  if (bytes <= 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  var value = bytes.toDouble();
  var unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit++;
  }
  return unit == 0
      ? '${value.toStringAsFixed(0)} B'
      : '${value.toStringAsFixed(1)} ${units[unit]}';
}

/// 毫秒时间戳 -> 本地时间 `HH:mm:ss`。
String formatClock(int timestampMs) {
  final t = DateTime.fromMillisecondsSinceEpoch(timestampMs);
  String two(int v) => v.toString().padLeft(2, '0');
  return '${two(t.hour)}:${two(t.minute)}:${two(t.second)}';
}
